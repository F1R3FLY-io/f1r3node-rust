use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use prost::Message;
use quote::ToTokens;
use serde::Serialize;
use syn::parse::Parser;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

mod sizing;

const CRATES: &[&str] = &[
    "block-storage",
    "casper",
    "comm",
    "crypto",
    "deploy-queue",
    "graphz",
    "models",
    "node",
    "rho-pure-eval",
    "rholang",
    "rspace++",
    "shared",
    "scripts/file-read-service",
];

#[derive(Debug, Serialize)]
struct Field {
    name: String,
    ty: String,
    references: BTreeSet<String>,
}

#[derive(Debug, Serialize)]
struct Entity {
    name: String,
    scope: String,
    source: String,
    line: usize,
    kind: String,
    fields: Vec<Field>,
    diagram: String,
}

fn tokens(value: &impl ToTokens) -> String { value.to_token_stream().to_string() }

fn cfg_value(meta: &syn::Meta) -> Option<bool> {
    match meta {
        syn::Meta::Path(p) if p.is_ident("test") => Some(false),
        syn::Meta::NameValue(n) if n.path.is_ident("feature") => {
            let value = tokens(&n.value);
            if ["\"test-internals\"", "\"test-utils\""].contains(&value.as_str()) {
                Some(false)
            } else {
                None
            }
        }
        syn::Meta::List(l) => {
            let args = l
                .parse_args_with(
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                )
                .ok()?;
            let values: Vec<_> = args.iter().map(cfg_value).collect();
            if l.path.is_ident("not") && values.len() == 1 {
                values[0].map(|v| !v)
            } else if l.path.is_ident("all") {
                if values.contains(&Some(false)) {
                    Some(false)
                } else if values.iter().all(|v| *v == Some(true)) {
                    Some(true)
                } else {
                    None
                }
            } else if l.path.is_ident("any") {
                if values.contains(&Some(true)) {
                    Some(true)
                } else if values.iter().all(|v| *v == Some(false)) {
                    Some(false)
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn excluded(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("test")
            || (a.path().is_ident("cfg")
                && a.parse_args::<syn::Meta>().ok().and_then(|m| cfg_value(&m)) == Some(false))
    })
}

#[derive(Default)]
struct References(BTreeSet<String>);

impl<'a> Visit<'a> for References {
    fn visit_type_path(&mut self, node: &'a syn::TypePath) {
        if let Some(segment) = node.path.segments.last() {
            self.0.insert(segment.ident.to_string());
        }
        visit::visit_type_path(self, node);
    }
}

fn field(name: String, ty: &syn::Type) -> Field {
    let mut refs = References::default();
    refs.visit_type(ty);
    Field {
        name,
        ty: tokens(ty),
        references: refs.0,
    }
}

struct Collector {
    source: String,
    scope: Vec<String>,
    entities: Vec<Entity>,
}

impl Collector {
    fn add(&mut self, name: String, line: usize, kind: &str, fields: Vec<Field>) {
        self.entities.push(Entity {
            name,
            scope: self.scope.join("::"),
            source: self.source.clone(),
            line,
            kind: kind.into(),
            fields,
            diagram: String::new(),
        });
    }
}

struct LazyDeclarations(Vec<(syn::Ident, syn::Type)>);

impl syn::parse::Parse for LazyDeclarations {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut values = Vec::new();
        while !input.is_empty() {
            let attrs = input.call(syn::Attribute::parse_outer)?;
            let _: syn::Visibility = input.parse()?;
            let _: syn::Token![static] = input.parse()?;
            let _: syn::Token![ref] = input.parse()?;
            let name = input.parse()?;
            let _: syn::Token![:] = input.parse()?;
            let ty = input.parse()?;
            let _: syn::Token![=] = input.parse()?;
            let _: syn::Expr = input.parse()?;
            let _: syn::Token![;] = input.parse()?;
            if !excluded(&attrs) {
                values.push((name, ty));
            }
        }
        Ok(Self(values))
    }
}

impl<'a> Visit<'a> for Collector {
    fn visit_item_macro(&mut self, node: &'a syn::ItemMacro) {
        if excluded(&node.attrs) {
            return;
        }
        let name = node.mac.path.segments.last().unwrap().ident.to_string();
        if name == "thread_local" {
            let parsed = syn::parse2::<syn::File>(node.mac.tokens.clone())
                .expect("thread_local declarations must parse");
            self.scope.push("thread_local".into());
            self.visit_file(&parsed);
            self.scope.pop();
        } else if name == "lazy_static" {
            let parsed = syn::parse2::<LazyDeclarations>(node.mac.tokens.clone())
                .expect("lazy_static declarations must parse");
            for (ident, ty) in parsed.0 {
                self.add(
                    ident.to_string(),
                    ident.span().start().line,
                    "lazy static",
                    vec![field("value".into(), &ty)],
                );
            }
        }
    }
    fn visit_item_mod(&mut self, node: &'a syn::ItemMod) {
        if excluded(&node.attrs) {
            return;
        }
        self.scope.push(node.ident.to_string());
        visit::visit_item_mod(self, node);
        self.scope.pop();
    }

    fn visit_item_fn(&mut self, node: &'a syn::ItemFn) {
        if excluded(&node.attrs) {
            return;
        }
        self.scope.push(node.sig.ident.to_string());
        visit::visit_item_fn(self, node);
        self.scope.pop();
    }

    fn visit_item_impl(&mut self, node: &'a syn::ItemImpl) {
        if excluded(&node.attrs) {
            return;
        }
        self.scope.push(tokens(&node.self_ty));
        visit::visit_item_impl(self, node);
        self.scope.pop();
    }

    fn visit_impl_item_fn(&mut self, node: &'a syn::ImplItemFn) {
        if excluded(&node.attrs) {
            return;
        }
        self.scope.push(node.sig.ident.to_string());
        visit::visit_impl_item_fn(self, node);
        self.scope.pop();
    }

    fn visit_item_struct(&mut self, node: &'a syn::ItemStruct) {
        if excluded(&node.attrs) {
            return;
        }
        let fields = node
            .fields
            .iter()
            .enumerate()
            .filter(|(_, f)| !excluded(&f.attrs))
            .map(|(i, f)| {
                field(
                    f.ident
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or(i.to_string()),
                    &f.ty,
                )
            })
            .collect();
        self.add(
            node.ident.to_string(),
            node.span().start().line,
            "struct",
            fields,
        );
    }

    fn visit_item_enum(&mut self, node: &'a syn::ItemEnum) {
        if excluded(&node.attrs) {
            return;
        }
        let mut fields = Vec::new();
        for v in &node.variants {
            if excluded(&v.attrs) {
                continue;
            }
            if v.fields.is_empty() {
                fields.push(Field {
                    name: v.ident.to_string(),
                    ty: v
                        .discriminant
                        .as_ref()
                        .map(|(_, expr)| format!("variant = {}", tokens(expr)))
                        .unwrap_or_else(|| "variant".into()),
                    references: BTreeSet::new(),
                });
            }
            for (i, f) in v.fields.iter().enumerate() {
                if !excluded(&f.attrs) {
                    fields.push(field(
                        format!(
                            "{}.{}",
                            v.ident,
                            f.ident
                                .as_ref()
                                .map(ToString::to_string)
                                .unwrap_or(i.to_string())
                        ),
                        &f.ty,
                    ));
                }
            }
        }
        self.add(
            node.ident.to_string(),
            node.span().start().line,
            "enum",
            fields,
        );
    }

    fn visit_item_type(&mut self, node: &'a syn::ItemType) {
        if !excluded(&node.attrs) {
            self.add(
                node.ident.to_string(),
                node.span().start().line,
                "alias",
                vec![field("value".into(), &node.ty)],
            );
        }
    }

    fn visit_item_static(&mut self, node: &'a syn::ItemStatic) {
        if !excluded(&node.attrs) {
            self.add(
                node.ident.to_string(),
                node.span().start().line,
                "static",
                vec![field("value".into(), &node.ty)],
            );
        }
    }
}

fn files(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path.file_name().unwrap().to_string_lossy();
        if [
            "test",
            "tests",
            "test_utils",
            "test_utils.rs",
            "test_tmpdir.rs",
            "scalapb",
            "target",
            "benches",
            "fixtures",
        ]
        .contains(&name.as_ref())
            || name.starts_with("bench_")
            || name.ends_with("_tests.rs")
            || name.ends_with("_test.rs")
            || name == "tests.rs"
        {
            continue;
        }
        if path.is_dir() {
            files(&path, extension, out)?;
        } else if path.extension().and_then(|x| x.to_str()) == Some(extension) {
            out.push(path);
        }
    }
    Ok(())
}

fn proto_message(
    msg: &prost_types::DescriptorProto,
    scope: &str,
    source: &str,
    out: &mut Vec<Entity>,
) {
    let name = msg.name.clone().unwrap();
    let full = format!("{scope}.{name}");
    let fields = msg
        .field
        .iter()
        .map(|f| {
            let ty = f
                .type_name
                .clone()
                .unwrap_or_else(|| format!("{:?}", f.r#type()));
            let repeated = f.label() == prost_types::field_descriptor_proto::Label::Repeated;
            let choice = f
                .oneof_index
                .map(|i| {
                    format!(
                        " oneof {}",
                        msg.oneof_decl[i as usize].name.as_deref().unwrap_or("?")
                    )
                })
                .unwrap_or_default();
            Field {
                name: format!(
                    "{} #{}{}",
                    f.name.as_deref().unwrap(),
                    f.number.unwrap(),
                    choice
                ),
                ty: format!("{}{}", if repeated { "repeated " } else { "" }, ty),
                references: f.type_name.iter().cloned().collect(),
            }
        })
        .collect();
    out.push(Entity {
        name,
        scope: scope.into(),
        source: source.into(),
        line: 0,
        kind: "protobuf".into(),
        fields,
        diagram: String::new(),
    });
    for child in &msg.nested_type {
        proto_message(child, &full, source, out);
    }
    for e in &msg.enum_type {
        proto_enum(e, &full, source, out);
    }
}

fn proto_enum(
    e: &prost_types::EnumDescriptorProto,
    scope: &str,
    source: &str,
    out: &mut Vec<Entity>,
) {
    out.push(Entity {
        name: e.name.clone().unwrap(),
        scope: scope.into(),
        source: source.into(),
        line: 0,
        kind: "protobuf enum".into(),
        diagram: String::new(),
        fields: e
            .value
            .iter()
            .map(|v| Field {
                name: v.name.clone().unwrap(),
                ty: v.number.unwrap().to_string(),
                references: BTreeSet::new(),
            })
            .collect(),
    });
}

fn protos(root: &Path, out: &mut Vec<Entity>) -> Result<(), Box<dyn Error>> {
    let mut paths = Vec::new();
    for name in ["models", "node", "comm"] {
        files(&root.join(name).join("src"), "proto", &mut paths)?;
    }
    paths.sort();
    let include: BTreeSet<_> = paths
        .iter()
        .map(|p| p.parent().unwrap().to_path_buf())
        .collect();
    let target = root.join("target/erd-tools/protos.bin");
    fs::create_dir_all(target.parent().unwrap())?;
    let mut cmd = Command::new(std::env::var_os("PROTOC").unwrap_or_else(|| "protoc".into()));
    cmd.arg(format!("--descriptor_set_out={}", target.display()));
    for dir in &include {
        cmd.arg(format!("-I{}", dir.display()));
    }
    for name in ["models", "node", "comm"] {
        cmd.arg(format!("-I{}", root.join(name).join("src").display()));
    }
    cmd.args(&paths);
    if !cmd.status()?.success() {
        return Err("protoc failed".into());
    }
    let set = prost_types::FileDescriptorSet::decode(fs::read(target)?.as_slice())?;
    for file in set.file {
        let path = paths
            .iter()
            .find(|p| p.ends_with(file.name.as_deref().unwrap()))
            .ok_or("descriptor source missing")?;
        let source = path.strip_prefix(root)?.to_string_lossy();
        let scope = format!(".{}", file.package.as_deref().unwrap_or(""));
        for msg in &file.message_type {
            proto_message(msg, &scope, &source, out);
        }
        for e in &file.enum_type {
            proto_enum(e, &scope, &source, out);
        }
    }
    Ok(())
}

fn escape(s: &str) -> String {
    s.replace('&', "&#38;")
        .replace('<', "&#60;")
        .replace('>', "&#62;")
        .replace('"', "&#34;")
        .replace('{', "&#123;")
        .replace('}', "&#125;")
        .replace('|', "&#124;")
        .replace('\\', "&#92;")
}

fn wrapped(s: &str) -> String {
    let mut lines = vec![String::new()];
    for word in s.split_whitespace() {
        let line = lines.last_mut().unwrap();
        if !line.is_empty() && line.len() + 1 + word.len() > 72 {
            lines.push(word.into());
        } else {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
    }
    lines
        .iter()
        .map(|l| escape(l))
        .collect::<Vec<_>>()
        .join("\\n    ")
}

fn output(path: &Path, value: &str, check: bool) -> Result<(), Box<dyn Error>> {
    if check {
        if fs::read_to_string(path).ok().as_deref() != Some(value) {
            return Err(format!("stale generated file: {}", path.display()).into());
        }
    } else {
        fs::write(path, value)?;
    }
    Ok(())
}

fn generate(root: &Path, entities: &mut [Entity], check: bool) -> Result<(), Box<dyn Error>> {
    let destination = root.join("docs/erd/memory");
    fs::create_dir_all(&destination)?;
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, e) in entities.iter().enumerate() {
        let crate_name = e
            .source
            .split("/src/")
            .next()
            .unwrap()
            .replace('/', "-")
            .replace('+', "p");
        groups.entry(crate_name).or_default().push(i);
    }
    let mut pages = Vec::new();
    for (group, indices) in groups {
        for (page, chunk) in indices.chunks(8).enumerate() {
            let stem = format!("catalog-{group}-{:03}", page + 1);
            for &i in chunk {
                entities[i].diagram = format!("{stem}.svg");
            }
            pages.push((stem, chunk.to_vec()));
        }
    }
    let mut names: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, e) in entities.iter().enumerate() {
        let name = if e.kind.starts_with("protobuf") {
            format!("{}.{}", e.scope, e.name)
        } else {
            e.name.clone()
        };
        names.entry(name).or_default().push(i);
    }
    let mut index = String::from("# Type Inventory\n\nGenerated from repository-owned production source. See [scope and notation](../README.md).\n\nEach declared field and enum payload appears in its linked diagram. Types are not disk tables.\n\n| Type | Kind | Source | Diagram |\n| --- | --- | --- | --- |\n");
    for e in entities.iter() {
        let location = if e.line == 0 {
            e.source.clone()
        } else {
            format!("{}:{}", e.source, e.line)
        };
        index.push_str(&format!(
            "| `{}` | {} | [{}](../../../{}) | [View]({}) / [SVG]({}) / [PlantUML]({}) |\n",
            e.name,
            e.kind,
            location,
            e.source,
            e.diagram.replace(".svg", ".md"),
            e.diagram,
            e.diagram.replace(".svg", ".puml")
        ));
    }
    let mut sizes = Vec::new();
    for (stem, indices) in &pages {
        let mut text = format!(
            "@startuml\n!include ../style.puml\ntitle {}\n",
            stem.replace('-', " ")
        );
        let mut edges = BTreeSet::new();
        let mut refs = BTreeSet::new();
        for &i in indices {
            let e = &entities[i];
            text.push_str(&format!(
                "entity \"{}\" as E{i} <<{}>> {{\n",
                escape(&e.name),
                e.kind
            ));
            let location = if e.line == 0 {
                e.source.clone()
            } else {
                format!("{}:{}", e.source, e.line)
            };
            text.push_str(&format!("  {}\n", wrapped(&location)));
            if !e.scope.is_empty() {
                text.push_str(&format!("  {}\n", wrapped(&e.scope)));
            }
            text.push_str("  --\n");
            for f in &e.fields {
                text.push_str(&format!(
                    "  {}\n",
                    wrapped(&format!("{} : {}", f.name, f.ty))
                ));
                for reference in &f.references {
                    if let Some(candidates) = names.get(reference) {
                        let local: Vec<_> = candidates
                            .iter()
                            .copied()
                            .filter(|j| {
                                entities[*j].source == e.source && entities[*j].scope == e.scope
                            })
                            .collect();
                        let target = if local.len() == 1 {
                            Some(local[0])
                        } else if candidates.len() == 1 {
                            Some(candidates[0])
                        } else {
                            None
                        };
                        if let Some(j) = target {
                            if i != j && indices.contains(&j) {
                                edges.insert(format!("E{i} --> E{j} : {}", escape(&f.name)));
                            } else if i != j {
                                refs.insert(format!(
                                    "{}: {} -> {} [{}]",
                                    e.name, f.name, entities[j].name, entities[j].diagram
                                ));
                            }
                        }
                    }
                }
            }
            text.push_str("}\n");
        }
        for edge in &edges {
            text.push_str(&format!("{edge}\n"));
        }
        if edges.is_empty() {
            for pair in indices.windows(2) {
                text.push_str(&format!("E{} -[hidden]down-> E{}\n", pair[0], pair[1]));
            }
        }
        text.push_str("legend bottom\nFields preserve source types. Arrows show local type references, not ownership or foreign keys.\nOther type definitions are listed in inventory.md. Enum rows are alternatives.\nendlegend\n@enduml\n");
        output(&destination.join(format!("{stem}.puml")), &text, check)?;
        let references = if refs.is_empty() {
            "No resolved cross-page references.".to_string()
        } else {
            refs.into_iter()
                .map(|r| format!("- `{r}`"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut sizing_text = String::from("## Cardinality and Size\n\n[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.\n");
        for &i in indices {
            let e = &entities[i];
            let fields: Vec<_> = e
                .fields
                .iter()
                .map(|f| sizing::field(&e.kind, &f.ty, &f.name))
                .collect();
            let count = sizing::entity_count(&e.kind, &e.scope);
            let size = sizing::entity_size(&e.kind);
            sizing_text.push_str(&format!("\n### {}\n\nCardinality: {count}.\n\nSize: {size}.\n\n| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |\n| --- | --- | --- | --- |\n", e.name));
            for (f, a) in e.fields.iter().zip(&fields) {
                let layout = match a.inline_bytes {
                    Some(n) if a.inline_layout != format!("{n} B") => {
                        format!("{n} B. {}", a.inline_layout)
                    }
                    _ => a.inline_layout.clone(),
                };
                sizing_text.push_str(&format!(
                    "| `{}` | {} | {} | {} |\n",
                    f.name.replace('|', "&#124;"),
                    a.cardinality,
                    layout,
                    a.additional_storage.replace('|', "&#124;")
                ));
            }
            if fields.is_empty() {
                sizing_text.push_str("| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |\n");
            }
            sizes.push(serde_json::json!({
                "entity": e.name, "source": e.source, "line": e.line, "scope": e.scope,
                "kind": e.kind, "page": format!("{stem}.md"), "cardinality": count, "data_size": size,
                "fields": e.fields.iter().zip(&fields).map(|(f, a)| serde_json::json!({"name": f.name, "type": f.ty, "assessment": a})).collect::<Vec<_>>()
            }));
        }
        output(&destination.join(format!("{stem}.md")), &format!("# {}\n\n[Inventory](inventory.md) | [PlantUML]({stem}.puml)\n\n![{}]({stem}.svg)\n\n{sizing_text}\n## Cross-Page References\n\n{}\n", stem.replace('-', " "), stem, references), check)?;
    }
    output(&destination.join("inventory.md"), &index, check)?;
    output(
        &destination.join("sizing.json"),
        &(serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1, "basis": "Source declarations, not a heap capture. See sizing.md",
            "generator_target": std::env::consts::ARCH, "pointer_bytes": std::mem::size_of::<usize>(),
            "entities": sizes,
        }))? + "\n"),
        check,
    )?;
    output(
        &destination.join("inventory.json"),
        &(serde_json::to_string_pretty(&entities)? + "\n"),
        check,
    )?;
    let expected: BTreeSet<_> = pages.iter().map(|p| format!("{}.puml", p.0)).collect();
    for entry in fs::read_dir(&destination)? {
        let p = entry?.path();
        let name = p.file_name().unwrap().to_string_lossy();
        if name.starts_with("catalog-")
            && name.ends_with(".puml")
            && !expected.contains(name.as_ref())
        {
            return Err(format!(
                "obsolete generated page: {} (remove after review)",
                p.display()
            )
            .into());
        }
    }
    println!(
        "{} types, {} rendered-diagram sources",
        entities.len(),
        pages.len()
    );
    Ok(())
}

fn check_links(root: &Path) -> Result<(), Box<dyn Error>> {
    let link = regex::Regex::new(r"!?\[[^\]]*\]\(([^\s)]+)\)")?;
    let mut paths = Vec::new();
    files(&root.join("docs/erd"), "md", &mut paths)?;
    for p in &paths {
        for m in link.captures_iter(&fs::read_to_string(p)?) {
            let target = m[1].split('#').next().unwrap();
            if !target.is_empty()
                && !target.contains("://")
                && !p.parent().unwrap().join(target).exists()
            {
                return Err(format!("broken reference in {}: {target}", p.display()).into());
            }
        }
    }
    println!("{} Markdown files: local references exist", paths.len());
    Ok(())
}

#[derive(Default)]
struct StoreNames {
    constants: BTreeMap<String, String>,
    names: BTreeSet<String>,
}

impl StoreNames {
    fn name(&self, expr: &syn::Expr) -> Option<String> {
        match expr {
            syn::Expr::Lit(l) => match &l.lit {
                syn::Lit::Str(s) => Some(s.value()),
                _ => None,
            },
            syn::Expr::MethodCall(m) => self.name(&m.receiver),
            syn::Expr::Path(p) => self
                .constants
                .get(&p.path.segments.last()?.ident.to_string())
                .cloned(),
            syn::Expr::Call(c) => c.args.first().and_then(|e| self.name(e)),
            _ => None,
        }
    }
}

impl<'a> Visit<'a> for StoreNames {
    fn visit_macro(&mut self, node: &'a syn::Macro) {
        if node.path.is_ident("vec") {
            let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
            let entries = parser
                .parse2(node.tokens.clone())
                .expect("store mapping vec must parse");
            for entry in &entries {
                self.visit_expr(entry);
            }
        }
    }
    fn visit_expr_call(&mut self, call: &'a syn::ExprCall) {
        if let syn::Expr::Path(path) = &*call.func {
            if tokens(&path.path) == "Db :: new" {
                if let Some(name) = call.args.first().and_then(|e| self.name(e)) {
                    self.names.insert(name);
                }
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_for_loop(&mut self, node: &'a syn::ExprForLoop) {
        if let syn::Expr::Array(array) = &*node.expr {
            for value in &array.elems {
                if let Some(name) = self.name(value) {
                    self.names.insert(name);
                }
            }
        }
        visit::visit_expr_for_loop(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'a syn::ExprMethodCall) {
        if node.method == "create_database" {
            if let Some(name) = node.args.last().and_then(|e| self.name(e)) {
                self.names.insert(name);
            }
        }
        visit::visit_expr_method_call(self, node);
    }
}

fn check_storage(root: &Path) -> Result<(), Box<dyn Error>> {
    let mut stores = StoreNames::default();
    let code =
        fs::read_to_string(root.join("block-storage/src/rust/deploy/key_value_deploy_storage.rs"))?;
    for item in syn::parse_file(&code)?.items {
        if let syn::Item::Const(c) = item {
            if let Some(value) = stores.name(&c.expr) {
                stores.constants.insert(c.ident.to_string(), value);
            }
        }
    }
    let code =
        fs::read_to_string(root.join("casper/src/rust/storage/rnode_key_value_store_manager.rs"))?;
    let parsed = syn::parse_file(&code)?;
    for item in &parsed.items {
        if let syn::Item::Fn(f) = item {
            if f.sig.ident == "rnode_db_mapping" {
                stores.visit_item_fn(f);
            }
        }
    }
    let queue = fs::read_to_string(root.join("deploy-queue/src/store.rs"))?;
    stores.visit_file(&syn::parse_file(&queue)?);
    let readme = fs::read_to_string(root.join("docs/erd/storage/README.md"))?;
    let sizes = fs::read_to_string(root.join("docs/erd/storage/sizing.md"))?;
    let mut diagrams = String::new();
    for entry in fs::read_dir(root.join("docs/erd/storage"))? {
        let p = entry?.path();
        if p.extension().and_then(|x| x.to_str()) == Some("puml") {
            diagrams.push_str(&fs::read_to_string(p)?);
        }
    }
    if stores.names.len() < 38 {
        return Err(format!(
            "storage mapping scan found {} names: {:?}",
            stores.names.len(),
            stores.names
        )
        .into());
    }
    for name in &stores.names {
        if !sizes.contains(&format!("| `{name}` |")) {
            return Err(format!("database missing cardinality and size assessment: {name}").into());
        }
        if !readme.contains(&format!("`{name}`")) {
            return Err(format!("undocumented database: {name}").into());
        }
        if !diagrams.contains(name)
            && ![
                "rspace-history",
                "rspace-roots",
                "rspace-cold",
                "eval-history",
                "eval-roots",
                "eval-cold",
                "rspace-channels",
            ]
            .contains(&name.as_str())
        {
            return Err(format!("database missing from diagrams: {name}").into());
        }
    }
    println!(
        "{} registered node and queue database names covered",
        stores.names.len()
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let check = std::env::args().any(|a| a == "--check");
    if std::env::args().any(|a| a == "--check-links") {
        check_storage(root)?;
        return check_links(root);
    }
    let mut entities = Vec::new();
    for name in CRATES {
        let mut paths = Vec::new();
        files(&root.join(name).join("src"), "rs", &mut paths)?;
        paths.sort();
        for path in paths {
            let source = path.strip_prefix(root)?.to_string_lossy().to_string();
            let code = fs::read_to_string(&path)?;
            let parsed = syn::parse_file(&code).map_err(|err| format!("{source}: {err}"))?;
            let mut collector = Collector {
                source,
                scope: Vec::new(),
                entities: Vec::new(),
            };
            if !excluded(&parsed.attrs) {
                collector.visit_file(&parsed);
            }
            entities.extend(collector.entities);
        }
    }
    protos(root, &mut entities)?;
    entities.sort_by(|a, b| {
        (&a.source, a.line, &a.scope, &a.name).cmp(&(&b.source, b.line, &b.scope, &b.name))
    });
    generate(root, &mut entities, check)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_types_and_skips_test_gates() {
        let source = r#"
            struct A { value: Option<Vec<B>> }
            enum B { Empty, Some { x: A } }
            type Alias = B;
            #[cfg(test)] mod tests { struct Hidden; }
            #[cfg(any(test, feature = "test-internals"))] struct HiddenToo;
            #[cfg(not(test))] struct Kept;
            #[cfg(feature = "production")] struct FeatureType;
            fn run() { struct Local; }
        "#;
        let mut c = Collector {
            source: "fixture.rs".into(),
            scope: vec![],
            entities: vec![],
        };
        c.visit_file(&syn::parse_file(source).unwrap());
        assert_eq!(
            c.entities
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            ["A", "B", "Alias", "Kept", "FeatureType", "Local"]
        );
        assert!(c.entities[0].fields[0].references.contains("B"));
        assert_eq!(c.entities.last().unwrap().scope, "run");
        assert_eq!(c.entities[1].fields[1].name, "Some.x");
    }

    #[test]
    fn output_check_detects_missing_and_stale_files() {
        let p = std::env::temp_dir().join(format!("erd-check-{}", std::process::id()));
        output(&p, "original", false).unwrap();
        assert!(output(&p, "original", true).is_ok());
        assert!(output(&p, "changed", true).is_err());
        fs::remove_file(&p).unwrap();
        assert!(output(&p, "original", true).is_err());
    }

    #[test]
    fn escapes_plantuml_delimiters() {
        assert_eq!(escape("Vec<T>"), "Vec&#60;T&#62;");
        assert!(!escape("{ x | y }").contains('{'));
    }

    #[test]
    fn catalogs_macro_statics_without_recording_initial_values() {
        let mut c = Collector {
            source: "fixture.rs".into(),
            scope: vec![],
            entities: vec![],
        };
        c.visit_file(
            &syn::parse_file(
                r#"
            thread_local! { static BUFFER: Vec<u8> = Vec::new(); }
            lazy_static! { pub static ref STATE: Option<A> = None; }
        "#,
            )
            .unwrap(),
        );
        assert_eq!(c.entities.len(), 2);
        assert_eq!(c.entities[0].scope, "thread_local");
        assert_eq!(c.entities[1].kind, "lazy static");
        assert!(c.entities[1].fields[0].references.contains("A"));
    }

    #[test]
    fn scans_direct_loop_and_queue_database_names() {
        let mut s = StoreNames::default();
        s.constants.insert("SEEDS".into(), "seeds".into());
        s.visit_file(
            &syn::parse_file(
                r#"
            fn mapping() {
                let mappings = vec![(Db::new("blocks".to_string(), None), config())];
                Db::new(SEEDS.to_string(), None);
                for name in ["history", "roots"] { Db::new(name.to_string(), None); }
                env.create_database(&mut tx, Some("records"));
            }
        "#,
            )
            .unwrap(),
        );
        assert_eq!(
            s.names,
            ["blocks", "seeds", "history", "roots", "records"]
                .into_iter()
                .map(String::from)
                .collect()
        );
    }

    #[test]
    fn protobuf_nested_messages_keep_identity_and_repetition() {
        let mut entities = Vec::new();
        let msg = prost_types::DescriptorProto {
            name: Some("Outer".into()),
            nested_type: vec![prost_types::DescriptorProto {
                name: Some("Inner".into()),
                ..Default::default()
            }],
            field: vec![prost_types::FieldDescriptorProto {
                name: Some("items".into()),
                number: Some(1),
                label: Some(3),
                r#type: Some(11),
                type_name: Some(".p.Outer.Inner".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        proto_message(&msg, ".p", "fixture.proto", &mut entities);
        assert_eq!(entities[0].fields[0].ty, "repeated .p.Outer.Inner");
        assert_eq!(entities[1].scope, ".p.Outer");
    }

    #[test]
    fn generated_assessment_covers_every_entity_and_field() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Collector {
            source: "sample/src/lib.rs".into(),
            scope: vec![],
            entities: vec![],
        };
        c.visit_file(&syn::parse_file("struct A { n: u64, values: Option<Vec<u8>> } enum B { Empty, Full(A) } type Alias = B;").unwrap());
        generate(dir.path(), &mut c.entities, false).unwrap();
        generate(dir.path(), &mut c.entities, true).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(
            &fs::read(dir.path().join("docs/erd/memory/sizing.json")).unwrap(),
        )
        .unwrap();
        let entries = parsed["entities"].as_array().unwrap();
        assert_eq!(entries.len(), c.entities.len());
        for (entry, entity) in entries.iter().zip(&c.entities) {
            assert_eq!(
                entry["fields"].as_array().unwrap().len(),
                entity.fields.len()
            );
            assert!(!entry["cardinality"].as_str().unwrap().is_empty());
            let page = fs::read_to_string(
                dir.path()
                    .join("docs/erd/memory")
                    .join(entry["page"].as_str().unwrap()),
            )
            .unwrap();
            assert!(page.contains(&format!("### {}", entity.name)));
        }
        assert_eq!(entries[0]["fields"][0]["assessment"]["inline_bytes"], 8);
        assert!(entries[0]["fields"][1]["assessment"]["inline_bytes"].is_null());
    }
}
