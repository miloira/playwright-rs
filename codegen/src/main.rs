//! Code generator for the Playwright protocol channel layer.
//!
//! Reads the protocol spec YAML files in `spec/` and emits a single Rust module
//! `src/protocol/generated.rs` containing:
//! - a struct for every `object`/`mixin` type and every interface initializer,
//! - an enum for every `enum` type (named and inline),
//! - typed parameter/result structs for every command,
//! - a channel wrapper per `interface` with an async method per command,
//! - a typed event enum per interface.
//!
//! Run with: `cargo run -p codegen`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use serde_yaml::Value;

const SPEC_FILES: &[&str] = &[
    "core", "serialized", "structs", "mixins", "handles", "network", "frame", "worker", "page",
    "browserContext", "browser", "browserType", "playwright", "api", "artifact", "tracing",
    "localUtils", "android", "electron",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Interface,
    Object,
    Enum,
    Mixin,
}

struct Gen {
    /// Top-level definitions by name, in load order.
    defs: BTreeMap<String, Value>,
    kinds: BTreeMap<String, Kind>,
    /// Emitted type definitions keyed by name (deduped).
    types: BTreeMap<String, String>,
    /// Channel wrapper impl blocks and event enums (protocol layer).
    impls: Vec<String>,
    /// Ergonomic wrapper blocks (api layer).
    ergo: Vec<String>,
    /// Ergonomic output structs for multi-field channel returns.
    ergo_types: BTreeMap<String, String>,
}

fn main() {
    let spec_dir = Path::new("spec");
    let mut defs: BTreeMap<String, Value> = BTreeMap::new();
    for file in SPEC_FILES {
        let path = spec_dir.join(format!("{file}.yml"));
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let doc: Value = serde_yaml::from_str(&text)
            .unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));
        if let Value::Mapping(map) = doc {
            for (k, v) in map {
                let name = k.as_str().expect("top-level key is a string").to_string();
                defs.insert(name, v);
            }
        }
    }

    let mut kinds = BTreeMap::new();
    for (name, def) in &defs {
        let kind = match type_tag(def).as_deref() {
            Some("interface") => Kind::Interface,
            Some("enum") => Kind::Enum,
            Some("mixin") => Kind::Mixin,
            _ => Kind::Object,
        };
        kinds.insert(name.clone(), kind);
    }

    let mut g = Gen {
        defs: defs.clone(),
        kinds,
        types: BTreeMap::new(),
        impls: Vec::new(),
        ergo: Vec::new(),
        ergo_types: BTreeMap::new(),
    };

    // Emit named types + protocol channel layer.
    let names: Vec<String> = defs.keys().cloned().collect();
    for name in &names {
        let def = defs.get(name).unwrap().clone();
        match g.kinds[name] {
            Kind::Enum => g.emit_enum(name, &def),
            Kind::Object | Kind::Mixin => {
                if let Some(props) = def.get("properties") {
                    g.emit_struct(name, props);
                } else {
                    // No properties: emit an empty struct.
                    g.push_type(name, format!("pub struct {name} {{}}"), true);
                }
            }
            Kind::Interface => g.emit_interface(name, &def),
        }
    }

    // Emit the ergonomic api layer for every interface.
    for name in &names {
        if g.kinds[name] == Kind::Interface {
            let def = defs.get(name).unwrap().clone();
            g.emit_ergo_interface(name, &def);
        }
    }

    g.write_output();
    g.write_ergo();
}

fn type_tag(def: &Value) -> Option<String> {
    def.get("type")
        .and_then(Value::as_str)
        .map(|s| s.trim_end_matches('?').to_string())
}

impl Gen {
    fn push_type(&mut self, name: &str, body: String, derive: bool) {
        if self.types.contains_key(name) {
            return;
        }
        let code = if derive {
            format!(
                "#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]\n{body}\n"
            )
        } else {
            format!("{body}\n")
        };
        self.types.insert(name.to_string(), code);
    }

    /// Resolves a property definition to a Rust type, emitting inline aux types
    /// as needed. Returns `(rust_type, optional, boxable)` where `boxable`
    /// marks a struct-like type that should be boxed when optional to break
    /// potential recursion.
    fn resolve(&mut self, path: &str, v: &Value) -> (String, bool, bool) {
        match v {
            Value::String(s) => {
                let optional = s.ends_with('?');
                let base = s.trim_end_matches('?');
                (self.map_named(base), optional, self.is_struct_like(base))
            }
            Value::Mapping(_) => {
                let tag = v
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("object")
                    .to_string();
                let optional = tag.ends_with('?');
                let base = tag.trim_end_matches('?');
                let mut boxable = false;
                let ty = match base {
                    "object" => {
                        let props = v.get("properties");
                        if let Some(props) = props {
                            self.emit_struct(path, props);
                            boxable = true;
                            path.to_string()
                        } else {
                            // Free-form object.
                            "serde_json::Value".to_string()
                        }
                    }
                    "array" => {
                        let items = v.get("items").expect("array needs items");
                        let (item_ty, _, _) = self.resolve(&format!("{path}Item"), items);
                        format!("Vec<{item_ty}>")
                    }
                    "enum" => {
                        self.emit_enum_from_literals(path, v);
                        path.to_string()
                    }
                    other => {
                        boxable = self.is_struct_like(other);
                        self.map_named(other)
                    }
                };
                (ty, optional, boxable)
            }
            _ => ("serde_json::Value".to_string(), false, false),
        }
    }

    /// Whether a named type is a plain struct (object/mixin), which may recurse
    /// and therefore needs boxing when held in an optional field.
    fn is_struct_like(&self, name: &str) -> bool {
        matches!(self.kinds.get(name), Some(Kind::Object) | Some(Kind::Mixin))
    }

    /// Maps a scalar keyword or a named type reference to a Rust type.
    fn map_named(&self, name: &str) -> String {
        match name {
            "string" => "String".to_string(),
            "int" => "i64".to_string(),
            "float" => "f64".to_string(),
            "boolean" => "bool".to_string(),
            "binary" => "Binary".to_string(),
            "json" => "serde_json::Value".to_string(),
            "Channel" => "OnlyGuid".to_string(),
            other => match self.kinds.get(other) {
                Some(Kind::Interface) => "OnlyGuid".to_string(),
                Some(_) => other.to_string(),
                None => "serde_json::Value".to_string(),
            },
        }
    }

    fn emit_enum(&mut self, name: &str, def: &Value) {
        self.emit_enum_from_literals(name, def);
    }

    fn emit_enum_from_literals(&mut self, name: &str, def: &Value) {
        let literals = match def.get("literals").and_then(Value::as_sequence) {
            Some(l) => l,
            None => return,
        };
        let mut variants = String::new();
        let mut seen = BTreeSet::new();
        let mut first = true;
        for lit in literals {
            let lit_str = yaml_scalar_to_string(lit);
            let variant = enum_variant_ident(&lit_str);
            if !seen.insert(variant.clone()) {
                continue;
            }
            if first {
                writeln!(variants, "    #[default]").unwrap();
                first = false;
            }
            writeln!(variants, "    #[serde(rename = {:?})]", lit_str).unwrap();
            writeln!(variants, "    {variant},").unwrap();
        }
        let body = format!("pub enum {name} {{\n{variants}}}");
        self.push_type(name, body, true);
    }

    fn emit_struct(&mut self, name: &str, props: &Value) {
        let map = match props.as_mapping() {
            Some(m) => m,
            None => {
                self.push_type(name, format!("pub struct {name} {{}}"), true);
                return;
            }
        };
        // Collect (ident, serde_attrs, type) first so recursive emits happen
        // before we finalize this struct's text.
        let mut fields: Vec<(String, String, String)> = Vec::new();
        for (k, v) in map {
            let key = k.as_str().unwrap_or_default().to_string();
            if key.starts_with("$mixin") {
                let mixin_name = v.as_str().expect("$mixin value is a type name");
                let field = key.trim_start_matches('$').to_string();
                fields.push((
                    safe_ident(&field),
                    "    #[serde(flatten)]\n".to_string(),
                    mixin_name.to_string(),
                ));
                continue;
            }
            let field_path = format!("{name}{}", pascal(&key));
            let (ty, optional, boxable) = self.resolve(&field_path, v);
            let ident = safe_ident(&key);
            let mut attrs = String::new();
            // Keep the wire name when the Rust ident differs (raw idents keep
            // their bare name automatically; snake vs camel is covered by
            // allow(non_snake_case)).
            let bare = ident.trim_start_matches("r#");
            if bare != key {
                attrs.push_str(&format!("    #[serde(rename = {:?})]\n", key));
            }
            if optional {
                attrs.push_str(
                    "    #[serde(skip_serializing_if = \"Option::is_none\", default)]\n",
                );
                let inner = if boxable { format!("Box<{ty}>") } else { ty };
                fields.push((ident, attrs, format!("Option<{inner}>")));
            } else {
                fields.push((ident, attrs, ty));
            }
        }
        let mut body = format!("pub struct {name} {{\n");
        for (ident, attrs, ty) in &fields {
            body.push_str(attrs);
            writeln!(body, "    pub {ident}: {ty},").unwrap();
        }
        body.push('}');
        self.push_type(name, body, true);
    }

    fn emit_interface(&mut self, name: &str, def: &Value) {
        // Merge inherited commands/events from `extends`.
        let mut commands: Vec<(String, Value)> = Vec::new();
        let mut events: Vec<(String, Value)> = Vec::new();
        if let Some(base) = def.get("extends").and_then(Value::as_str) {
            if let Some(base_def) = self.defs.get(base).cloned() {
                collect_members(&base_def, "commands", &mut commands);
                collect_members(&base_def, "events", &mut events);
            }
        }
        collect_members(def, "commands", &mut commands);
        collect_members(def, "events", &mut events);

        // Initializer struct + accessor.
        let init_accessor = if let Some(init) = def.get("initializer") {
            let init_name = format!("{name}Initializer");
            self.emit_struct(&init_name, init);
            format!(
                "    pub async fn initializer(&self) -> crate::error::Result<{init_name}> {{\n        Ok(serde_json::from_value(self.conn.initializer(&self.guid).await?)?)\n    }}\n"
            )
        } else {
            String::new()
        };

        // Methods.
        let mut methods = String::new();
        methods.push_str(&init_accessor);
        for (cmd, cdef) in &commands {
            let method_ident = safe_ident(cmd);
            let params_val = cdef.get("parameters");
            let returns_val = cdef.get("returns");
            let params_ty = params_val.map(|p| {
                let ty = format!("{name}{}Params", pascal(cmd));
                self.emit_struct(&ty, p);
                ty
            });
            let result_ty = returns_val.map(|r| {
                let ty = format!("{name}{}Result", pascal(cmd));
                self.emit_struct(&ty, r);
                ty
            });

            let (sig_param, params_expr) = match &params_ty {
                Some(ty) => (
                    format!(", params: {ty}"),
                    "serde_json::to_value(params)?".to_string(),
                ),
                None => (String::new(), "serde_json::json!({})".to_string()),
            };
            let ret_ty = result_ty.clone().unwrap_or_else(|| "()".to_string());

            let body = if result_ty.is_some() {
                format!(
                    "        let __v = self.conn.send(&self.guid, {cmd:?}, {params_expr}).await?;\n        Ok(serde_json::from_value(__v)?)\n"
                )
            } else {
                format!(
                    "        self.conn.send(&self.guid, {cmd:?}, {params_expr}).await?;\n        Ok(())\n"
                )
            };
            writeln!(
                methods,
                "    pub async fn {method_ident}(&self{sig_param}) -> crate::error::Result<{ret_ty}> {{\n{body}    }}"
            )
            .unwrap();
        }

        // Event param structs + event enum.
        let mut event_variants = String::new();
        let mut event_match = String::new();
        for (ev, edef) in &events {
            let variant = pascal(ev);
            if let Some(params) = edef.get("parameters") {
                let ty = format!("{name}{}EventParams", pascal(ev));
                self.emit_struct(&ty, params);
                writeln!(event_variants, "    {variant}({ty}),").unwrap();
                writeln!(
                    event_match,
                    "            {ev:?} => match serde_json::from_value(params.clone()) {{ Ok(p) => {name}ChannelEvent::{variant}(p), Err(_) => {name}ChannelEvent::Unknown {{ method: method.to_string(), params }} }},"
                )
                .unwrap();
            } else {
                writeln!(event_variants, "    {variant},").unwrap();
                writeln!(event_match, "            {ev:?} => {name}ChannelEvent::{variant},").unwrap();
            }
        }
        let event_enum = format!(
            "#[derive(Debug, Clone)]\npub enum {name}ChannelEvent {{\n{event_variants}    Unknown {{ method: String, params: serde_json::Value }},\n}}\n\nimpl {name}ChannelEvent {{\n    pub fn parse(method: &str, params: serde_json::Value) -> {name}ChannelEvent {{\n        match method {{\n{event_match}            _ => {name}ChannelEvent::Unknown {{ method: method.to_string(), params }},\n        }}\n    }}\n}}\n"
        );

        let wrapper = format!(
            "#[derive(Clone)]\npub struct {name} {{\n    pub(crate) conn: crate::connection::Connection,\n    pub guid: String,\n}}\n\nimpl {name} {{\n    pub fn from_channel(conn: crate::connection::Connection, guid: String) -> {name} {{\n        {name} {{ conn, guid }}\n    }}\n\n    pub fn guid(&self) -> &str {{\n        &self.guid\n    }}\n\n    pub async fn subscribe(&self) -> tokio::sync::mpsc::UnboundedReceiver<crate::connection::Event> {{\n        self.conn.subscribe(&self.guid).await\n    }}\n\n{methods}}}\n\n{event_enum}"
        );
        self.impls.push(wrapper);
    }

    /// Emits an ergonomic wrapper for an interface: snake_case methods that
    /// return typed channel wrappers, unwrap single-field results, and expose
    /// a typed event stream.
    fn emit_ergo_interface(&mut self, name: &str, def: &Value) {
        let mut commands: Vec<(String, Value)> = Vec::new();
        let mut events: Vec<(String, Value)> = Vec::new();
        if let Some(base) = def.get("extends").and_then(Value::as_str) {
            if let Some(base_def) = self.defs.get(base).cloned() {
                collect_members(&base_def, "commands", &mut commands);
                collect_members(&base_def, "events", &mut events);
            }
        }
        collect_members(def, "commands", &mut commands);
        collect_members(def, "events", &mut events);

        let mut methods = String::new();

        if def.get("initializer").is_some() {
            writeln!(
                methods,
                "    /// The object's initializer, sent by the driver on creation.\n    pub async fn initializer(&self) -> crate::error::Result<crate::protocol::{name}Initializer> {{\n        Ok(serde_json::from_value(self.conn.initializer(&self.guid).await?)?)\n    }}"
            )
            .unwrap();
        }

        for (cmd, cdef) in &commands {
            let m = safe_ident(&snake(cmd));
            let params_present = cdef.get("parameters").is_some();
            let (sig_param, params_expr) = if params_present {
                (
                    format!(", params: crate::protocol::{name}{}Params", pascal(cmd)),
                    "serde_json::to_value(params)?".to_string(),
                )
            } else {
                (String::new(), "serde_json::json!({})".to_string())
            };

            let (ret_ty, tail) = match cdef.get("returns") {
                None => ("()".to_string(), "let _ = __v; Ok(())".to_string()),
                Some(r) => {
                    let fields: Vec<(String, Value)> = r
                        .as_mapping()
                        .map(|m| {
                            m.iter()
                                .map(|(k, v)| {
                                    (k.as_str().unwrap_or_default().to_string(), v.clone())
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    if fields.is_empty() {
                        ("()".to_string(), "let _ = __v; Ok(())".to_string())
                    } else if fields.len() == 1 {
                        let (key, val) = &fields[0];
                        let path = format!("{name}{}Result{}", pascal(cmd), pascal(key));
                        let (ty, expr) = self.classify_field(&path, key, val);
                        (ty, format!("Ok({expr})"))
                    } else {
                        // Multiple fields: generate an ergonomic output struct
                        // whose channel fields are typed wrappers.
                        let out_name = format!("{name}{}Out", pascal(cmd));
                        let mut decls = String::new();
                        let mut builds = String::new();
                        for (key, val) in &fields {
                            let path = format!("{out_name}{}", pascal(key));
                            let (ty, expr) = self.classify_field(&path, key, val);
                            let id = safe_ident(key);
                            writeln!(decls, "    pub {id}: {ty},").unwrap();
                            writeln!(builds, "            {id}: {expr},").unwrap();
                        }
                        let struct_code = format!(
                            "/// Output of [`{name}::{}`].\n#[derive(Clone)]\npub struct {out_name} {{\n{decls}}}\n",
                            safe_ident(&snake(cmd))
                        );
                        self.ergo_types.insert(out_name.clone(), struct_code);
                        (out_name.clone(), format!("Ok({out_name} {{\n{builds}        }})"))
                    }
                }
            };

            writeln!(
                methods,
                "    pub async fn {m}(&self{sig_param}) -> crate::error::Result<{ret_ty}> {{\n        let __v = self.conn.send(&self.guid, {cmd:?}, {params_expr}).await?;\n        {tail}\n    }}"
            )
            .unwrap();
        }

        let events_method = if events.is_empty() {
            String::new()
        } else {
            format!(
                "    /// A stream of typed events emitted by this object.\n    pub async fn events(&self) -> tokio::sync::mpsc::UnboundedReceiver<crate::protocol::{name}ChannelEvent> {{\n        let mut __raw = self.conn.subscribe(&self.guid).await;\n        let (__tx, __rx) = tokio::sync::mpsc::unbounded_channel();\n        tokio::spawn(async move {{\n            while let Some((__m, __p)) = __raw.recv().await {{\n                if __tx.send(crate::protocol::{name}ChannelEvent::parse(&__m, __p)).is_err() {{\n                    break;\n                }}\n            }}\n        }});\n        __rx\n    }}\n"
            )
        };

        let block = format!(
            "/// Ergonomic wrapper for the `{name}` channel.\n#[derive(Clone)]\npub struct {name} {{\n    pub(crate) conn: crate::connection::Connection,\n    pub guid: String,\n}}\n\nimpl {name} {{\n    pub fn from_channel(conn: crate::connection::Connection, guid: String) -> {name} {{\n        {name} {{ conn, guid }}\n    }}\n\n    /// The guid of the underlying remote object.\n    pub fn guid(&self) -> &str {{\n        &self.guid\n    }}\n\n    /// A clone of the underlying connection.\n    pub fn connection(&self) -> crate::connection::Connection {{\n        self.conn.clone()\n    }}\n\n    /// The low-level generated protocol channel for this object.\n    pub fn channel(&self) -> crate::protocol::{name} {{\n        crate::protocol::{name}::from_channel(self.conn.clone(), self.guid.clone())\n    }}\n\n{methods}{events_method}}}\n"
        );
        self.ergo.push(block);
    }

    /// Classifies a return field and produces `(ret_type, expr)` where `expr`
    /// evaluates to a value of `ret_type` (and may use `?`), reading from a
    /// local `__v: serde_json::Value`.
    fn classify_field(&mut self, path: &str, key: &str, v: &Value) -> (String, String) {
        match v {
            Value::String(s) => {
                let optional = s.ends_with('?');
                let base = s.trim_end_matches('?');
                if let Some(w) = self.interface_ref(base) {
                    self.channel_expr(&w, key, optional, false)
                } else {
                    let ty = self.map_named(base);
                    self.plain_expr(&ty, key, optional)
                }
            }
            Value::Mapping(_) => {
                let tag = v.get("type").and_then(Value::as_str).unwrap_or("object");
                let base = tag.trim_end_matches('?');
                if base == "array" {
                    if let Some(items) = v.get("items").and_then(Value::as_str) {
                        let ib = items.trim_end_matches('?');
                        if let Some(w) = self.interface_ref(ib) {
                            return self.channel_expr(&w, key, false, true);
                        }
                    }
                    let (ty, _, _) = self.resolve(path, v);
                    self.plain_expr(&ty, key, false)
                } else {
                    let (ty, optional, _) = self.resolve(path, v);
                    self.plain_expr(&ty, key, optional)
                }
            }
            _ => self.plain_expr("serde_json::Value", key, false),
        }
    }

    fn interface_ref(&self, base: &str) -> Option<String> {
        if base == "Channel" {
            return None;
        }
        match self.kinds.get(base) {
            Some(Kind::Interface) => Some(base.to_string()),
            _ => None,
        }
    }

    fn channel_expr(&self, wrapper: &str, key: &str, optional: bool, array: bool) -> (String, String) {
        if array {
            let expr = format!(
                "{{ let mut __out = Vec::new(); if let Some(__arr) = __v.get({key:?}).and_then(|v| v.as_array()) {{ for __it in __arr {{ if let Some(__g) = __it.get(\"guid\").and_then(|v| v.as_str()) {{ __out.push({wrapper}::from_channel(self.conn.clone(), __g.to_string())); }} }} }} __out }}"
            );
            (format!("Vec<{wrapper}>"), expr)
        } else if optional {
            let expr = format!(
                "__v.get({key:?}).and_then(|v| v.get(\"guid\")).and_then(|v| v.as_str()).map(|__g| {wrapper}::from_channel(self.conn.clone(), __g.to_string()))"
            );
            (format!("Option<{wrapper}>"), expr)
        } else {
            let expr = format!(
                "{{ let __g = __v.get({key:?}).and_then(|v| v.get(\"guid\")).and_then(|v| v.as_str()).ok_or_else(|| crate::error::Error::Protocol(format!(\"missing channel field {key}\")))?; {wrapper}::from_channel(self.conn.clone(), __g.to_string()) }}"
            );
            (wrapper.to_string(), expr)
        }
    }

    fn plain_expr(&self, ty: &str, key: &str, optional: bool) -> (String, String) {
        let ret = if optional { format!("Option<{ty}>") } else { ty.to_string() };
        let expr = format!(
            "serde_json::from_value(__v.get({key:?}).cloned().unwrap_or(serde_json::Value::Null))?"
        );
        (ret, expr)
    }

    fn write_ergo(&self) {
        let mut out = String::new();
        out.push_str(
            "// @generated by `cargo run -p codegen`. Do not edit by hand.\n//\n// Ergonomic api layer: typed, snake_case wrappers over every protocol\n// interface, with channel-typed returns, unwrapped single-field results and\n// typed event streams. Built on top of `crate::protocol`.\n#![allow(clippy::all)]\n#![allow(non_snake_case)]\n\n// Protocol types (params/results/structs/enums) are in scope here. Local\n// ergonomic wrapper structs with the same name as a protocol channel shadow\n// the glob import, which is exactly what we want.\n#[allow(unused_imports)]\nuse crate::protocol::*;\n\n",
        );
        for code in self.ergo_types.values() {
            out.push_str(code);
            out.push('\n');
        }
        for block in &self.ergo {
            out.push_str(block);
            out.push('\n');
        }
        let path = Path::new("src/api/generated.rs");
        fs::write(path, out).unwrap();
        eprintln!("wrote {} ({} ergonomic wrappers)", path.display(), self.ergo.len());
    }

    fn write_output(&self) {
        let mut out = String::new();
        out.push_str(
            "// @generated by `cargo run -p codegen`. Do not edit by hand.\n//\n// Typed channel layer for the Playwright protocol, covering every interface,\n// command, event and type defined in the protocol spec.\n#![allow(non_snake_case)]\n#![allow(non_camel_case_types)]\n#![allow(clippy::all)]\n\n",
        );
        out.push_str(
            "/// Base64-encoded binary payload on the wire.\npub type Binary = String;\n\n/// A reference to a remote channel object, carried as its guid.\n#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]\npub struct OnlyGuid {\n    pub guid: String,\n}\n\n",
        );
        for code in self.types.values() {
            out.push_str(code);
            out.push('\n');
        }
        for code in &self.impls {
            out.push_str(code);
            out.push('\n');
        }

        let dir = Path::new("src/protocol");
        fs::create_dir_all(dir).unwrap();
        let path = dir.join("generated.rs");
        fs::write(&path, out).unwrap();
        eprintln!(
            "wrote {} ({} types, {} interfaces)",
            path.display(),
            self.types.len(),
            self.impls.len()
        );
    }
}

fn collect_members(def: &Value, key: &str, out: &mut Vec<(String, Value)>) {
    if let Some(map) = def.get(key).and_then(Value::as_mapping) {
        for (k, v) in map {
            let name = k.as_str().unwrap_or_default().to_string();
            // Later definitions (the interface's own) override inherited ones.
            if let Some(slot) = out.iter_mut().find(|(n, _)| *n == name) {
                slot.1 = v.clone();
            } else {
                out.push((name, v.clone()));
            }
        }
    }
}

fn yaml_scalar_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => "null".to_string(),
        _ => String::new(),
    }
}

/// Builds a valid, unique-ish Rust enum variant identifier from a literal.
fn enum_variant_ident(lit: &str) -> String {
    let mapped = match lit {
        "null" => "Null",
        "undefined" => "Undefined",
        "NaN" => "NaN",
        "Infinity" => "Infinity",
        "-Infinity" => "NegInfinity",
        "-0" => "NegZero",
        _ => "",
    };
    if !mapped.is_empty() {
        return mapped.to_string();
    }
    let p = pascal(lit);
    if p.is_empty() {
        "Empty".to_string()
    } else if p.chars().next().unwrap().is_ascii_digit() {
        format!("V{p}")
    } else {
        p
    }
}

/// snake_case a lowerCamelCase protocol method name.
fn snake(s: &str) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// PascalCase a string, splitting on non-alphanumeric boundaries.
fn pascal(s: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            if upper {
                out.extend(ch.to_uppercase());
                upper = false;
            } else {
                out.push(ch);
            }
        } else {
            upper = true;
        }
    }
    out
}

/// Returns a valid Rust identifier, using a raw identifier for keywords.
fn safe_ident(s: &str) -> String {
    const KEYWORDS: &[&str] = &[
        "as", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false",
        "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
        "ref", "return", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
        "where", "while", "async", "await", "abstract", "become", "box", "do", "final", "macro",
        "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
    ];
    // These keywords cannot be raw identifiers; fall back to a suffix.
    const NON_RAW: &[&str] = &["self", "Self", "crate", "super"];
    if NON_RAW.contains(&s) {
        return format!("{s}_");
    }
    if KEYWORDS.contains(&s) {
        return format!("r#{s}");
    }
    s.to_string()
}
