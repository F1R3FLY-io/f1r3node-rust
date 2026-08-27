//! Module and import resolution.
//!
//! D6: imports resolve by URL. D7: resolution happens at compile time, in a
//! client toolchain, so the node never performs it. The [`Resolver`] trait is
//! the seam: `FileResolver` covers local development, an HTTP resolver covers
//! the on-chain case, and `MemResolver` covers tests.
//!
//! Plan 9.1 is unresolved and visible here: a bare URL is not a reproducible
//! reference. When a content hash is added to the surface, it is checked in
//! [`Program::load`], and [`Program::lockfile`] is what a build records.

use crate::ast::*;
use crate::diag::{Diag, DiagKind};
use crate::lex::Span;
use crate::parse::parse_module;
use std::collections::HashMap;

pub trait Resolver {
    fn fetch(&self, url: &str) -> Result<String, String>;
    /// Resolve a possibly relative reference against the importing document.
    fn join(&self, base: &str, url: &str) -> String {
        if url.contains("://") || base.is_empty() {
            return url.to_string();
        }
        match base.rfind('/') {
            Some(i) => format!("{}{}", &base[..=i], url),
            None => url.to_string(),
        }
    }
}

/// In-memory resolver: the test and embedded-corpus case.
#[derive(Default)]
pub struct MemResolver {
    pub files: HashMap<String, String>,
}

impl MemResolver {
    pub fn new() -> MemResolver {
        MemResolver::default()
    }
    pub fn with(mut self, url: &str, src: &str) -> MemResolver {
        self.files.insert(url.to_string(), src.to_string());
        self
    }
}

impl Resolver for MemResolver {
    fn fetch(&self, url: &str) -> Result<String, String> {
        self.files
            .get(url)
            .cloned()
            .ok_or_else(|| format!("no such module: {url}"))
    }
    fn join(&self, _base: &str, url: &str) -> String {
        url.to_string()
    }
}

/// Local filesystem resolver, for development.
pub struct FileResolver {
    pub root: std::path::PathBuf,
}

impl Resolver for FileResolver {
    fn fetch(&self, url: &str) -> Result<String, String> {
        let p = self.root.join(url.trim_start_matches("file://"));
        std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))
    }
}

/// A resolved import graph plus the entry module.
pub struct Program {
    entry: String,
    modules: HashMap<String, ModuleFile>,
    /// url -> byte length, standing in for the content hash of plan 9.1.
    sizes: HashMap<String, usize>,
}

impl Program {
    pub fn load(entry_url: &str, r: &dyn Resolver) -> Result<Program, Diag> {
        let mut modules = HashMap::new();
        let mut sizes = HashMap::new();
        let mut stack = vec![entry_url.to_string()];
        let mut in_progress: Vec<String> = Vec::new();

        while let Some(url) = stack.pop() {
            if modules.contains_key(&url) {
                continue;
            }
            if in_progress.contains(&url) {
                return Err(Diag::new(
                    DiagKind::Resolution,
                    format!("import cycle through {url}"),
                    Span { line: 0, col: 0 },
                ));
            }
            in_progress.push(url.clone());

            let src = r.fetch(&url).map_err(|e| {
                Diag::new(
                    DiagKind::Resolution,
                    format!("cannot resolve import {url}: {e}"),
                    Span { line: 0, col: 0 },
                )
            })?;
            sizes.insert(url.clone(), src.len());
            let m = parse_module(&src)?;
            for imp in &m.imports {
                let child = r.join(&url, imp.url());
                if !modules.contains_key(&child) {
                    stack.push(child);
                }
            }
            modules.insert(url.clone(), m);
        }

        // Cycle detection proper: the import graph must be acyclic.
        detect_cycle(entry_url, &modules, r)?;

        Ok(Program {
            entry: entry_url.to_string(),
            modules,
            sizes,
        })
    }

    pub fn entry_url(&self) -> &str {
        &self.entry
    }
    pub fn entry_module(&self) -> &ModuleFile {
        &self.modules[&self.entry]
    }
    pub fn module(&self, url: &str) -> Option<&ModuleFile> {
        self.modules.get(url)
    }

    /// Plan 9.1: what a reproducible build would record.
    pub fn lockfile(&self) -> Vec<(String, usize)> {
        let mut v: Vec<(String, usize)> =
            self.sizes.iter().map(|(k, s)| (k.clone(), *s)).collect();
        v.sort();
        v
    }

    /// Resolve a dotted path to a theory declaration and the url of the module
    /// that owns it.
    pub fn lookup(
        &self,
        path: &DottedPath,
        here: &str,
        span: Span,
    ) -> Result<(TheoryDecl, String), Diag> {
        let m = self.modules.get(here).ok_or_else(|| {
            Diag::new(DiagKind::Resolution, format!("unknown module {here}"), span)
        })?;

        if path.is_simple() {
            let name = path.last();
            if let Some(d) = m.decls.iter().find(|d| d.name == name) {
                return Ok((d.clone(), here.to_string()));
            }
            // `import Name from "<url>"`
            for imp in &m.imports {
                if let Import::FromModule { name: n, url, .. } = imp {
                    if n == name {
                        let child = self.child_url(here, url);
                        if let Some(cm) = self.modules.get(&child) {
                            if let Some(d) = cm.decls.iter().find(|d| d.name == name) {
                                return Ok((d.clone(), child));
                            }
                        }
                    }
                }
            }
            return Err(Diag::new(
                DiagKind::Resolution,
                format!("no theory named `{name}` in scope"),
                span,
            ));
        }

        // Qualified: alias . Name
        let alias = &path.0[0];
        let rest = path.0[1..].join(".");
        for imp in &m.imports {
            if let Import::ModuleAs {
                url, alias: a, ..
            } = imp
            {
                if a == alias {
                    let child = self.child_url(here, url);
                    let cm = self.modules.get(&child).ok_or_else(|| {
                        Diag::new(
                            DiagKind::Resolution,
                            format!("module {child} was not loaded"),
                            span,
                        )
                    })?;
                    if let Some(d) = cm.decls.iter().find(|d| d.name == rest) {
                        return Ok((d.clone(), child));
                    }
                    return Err(Diag::new(
                        DiagKind::Resolution,
                        format!("module `{alias}` has no theory `{rest}`"),
                        span,
                    ));
                }
            }
        }
        Err(Diag::new(
            DiagKind::Resolution,
            format!("no import aliased `{alias}`"),
            span,
        ))
    }

    fn child_url(&self, base: &str, url: &str) -> String {
        if self.modules.contains_key(url) {
            return url.to_string();
        }
        match base.rfind('/') {
            Some(i) => {
                let j = format!("{}{}", &base[..=i], url);
                if self.modules.contains_key(&j) {
                    j
                } else {
                    url.to_string()
                }
            }
            None => url.to_string(),
        }
    }
}

fn detect_cycle(
    entry: &str,
    modules: &HashMap<String, ModuleFile>,
    r: &dyn Resolver,
) -> Result<(), Diag> {
    fn go(
        url: &str,
        modules: &HashMap<String, ModuleFile>,
        r: &dyn Resolver,
        path: &mut Vec<String>,
        done: &mut Vec<String>,
    ) -> Result<(), Diag> {
        if done.contains(&url.to_string()) {
            return Ok(());
        }
        if path.contains(&url.to_string()) {
            return Err(Diag::new(
                DiagKind::Resolution,
                format!("import cycle: {} -> {url}", path.join(" -> ")),
                Span { line: 0, col: 0 },
            ));
        }
        path.push(url.to_string());
        if let Some(m) = modules.get(url) {
            for imp in &m.imports {
                let child = r.join(url, imp.url());
                let child = if modules.contains_key(&child) {
                    child
                } else {
                    imp.url().to_string()
                };
                go(&child, modules, r, path, done)?;
            }
        }
        path.pop();
        done.push(url.to_string());
        Ok(())
    }
    let mut path = Vec::new();
    let mut done = Vec::new();
    go(entry, modules, r, &mut path, &mut done)
}
