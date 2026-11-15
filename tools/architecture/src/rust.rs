//! Resolves the source-level module and import forms used by this workspace.
use crate::{graph::Dependency, rules};
use proc_macro2::{Span, TokenStream, TokenTree};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use syn::{
    Item, UseTree,
    spanned::Spanned,
    visit::{self, Visit},
};

type Name = Vec<String>;

#[derive(Clone, Default)]
struct Imports {
    aliases: BTreeMap<String, Name>,
    globs: Vec<Name>,
}

struct Module {
    file: PathBuf,
    owner: Name,
    items: Vec<Item>,
    imports: Imports,
    symbols: BTreeSet<String>,
}

#[derive(Default)]
pub struct Sources {
    modules: BTreeMap<Name, Module>,
    dependencies: BTreeMap<String, BTreeMap<String, String>>,
    issues: BTreeSet<String>,
}

fn identifier(id: &syn::Ident) -> String {
    id.to_string().trim_start_matches("r#").to_owned()
}
fn path(path: &syn::Path) -> Name {
    path.segments
        .iter()
        .map(|part| identifier(&part.ident))
        .collect()
}

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
            || (attr.path().is_ident("cfg")
                && attr
                    .parse_args::<syn::Path>()
                    .is_ok_and(|path| path.is_ident("test")))
    })
}

fn use_paths(tree: &UseTree, prefix: Name, output: &mut Vec<(Option<String>, Name)>) {
    match tree {
        UseTree::Path(part) => {
            let mut prefix = prefix;
            prefix.push(identifier(&part.ident));
            use_paths(&part.tree, prefix, output);
        }
        UseTree::Name(name) => {
            let id = identifier(&name.ident);
            let mut target = prefix;
            if id != "self" {
                target.push(id.clone());
            }
            let alias = if id == "self" {
                target.last().cloned()
            } else {
                Some(id)
            };
            output.push((alias, target));
        }
        UseTree::Rename(rename) => {
            let mut target = prefix;
            if rename.ident != "self" {
                target.push(identifier(&rename.ident));
            }
            output.push((Some(identifier(&rename.rename)), target));
        }
        UseTree::Glob(_) => output.push((None, prefix)),
        UseTree::Group(group) => {
            for item in &group.items {
                use_paths(item, prefix.clone(), output);
            }
        }
    }
}

impl Imports {
    fn collect(items: impl IntoIterator<Item = Item>) -> Self {
        let mut output = Self::default();
        for item in items {
            match item {
                Item::Use(item) if !test_only(&item.attrs) => {
                    let mut paths = vec![];
                    use_paths(&item.tree, vec![], &mut paths);
                    for (alias, target) in paths {
                        if let Some(alias) = alias {
                            output.aliases.insert(alias, target);
                        } else {
                            output.globs.push(target);
                        }
                    }
                }
                Item::Type(item) if !test_only(&item.attrs) => {
                    if let syn::Type::Path(target) = item.ty.as_ref() {
                        output
                            .aliases
                            .insert(identifier(&item.ident), path(&target.path));
                    }
                }
                Item::ExternCrate(item) => {
                    let alias = item
                        .rename
                        .as_ref()
                        .map(|(_, name)| name)
                        .unwrap_or(&item.ident);
                    output
                        .aliases
                        .insert(identifier(alias), vec![identifier(&item.ident)]);
                }
                _ => {}
            }
        }
        output
    }
}

impl Sources {
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    pub fn load_crate(
        &mut self,
        name: &str,
        file: &Path,
        dependencies: &[Dependency],
    ) -> Result<(), String> {
        let name = name.replace('-', "_");
        self.dependencies.insert(
            name.clone(),
            dependencies
                .iter()
                .map(|dependency| {
                    (
                        dependency.alias.clone(),
                        dependency.package.replace('-', "_"),
                    )
                })
                .collect(),
        );
        let root = file
            .parent()
            .ok_or("Library source has no parent.")?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        self.load_file(vec![name], file, &root, &mut BTreeSet::new())
    }

    fn load_file(
        &mut self,
        name: Name,
        file: &Path,
        root: &Path,
        stack: &mut BTreeSet<PathBuf>,
    ) -> Result<(), String> {
        let file = file
            .canonicalize()
            .map_err(|error| format!("{}: {error}", file.display()))?;
        if !stack.insert(file.clone()) {
            return Err(format!("{}: recursive module ownership", file.display()));
        }
        let relative = file.strip_prefix(root).map_err(|_| {
            format!(
                "{}: module source escapes its crate source directory",
                file.display()
            )
        })?;
        let mut owner = vec![name[0].clone()];
        for part in relative.components() {
            let part = part.as_os_str().to_string_lossy();
            if !matches!(part.as_ref(), "lib.rs" | "mod.rs") {
                owner.push(part.trim_end_matches(".rs").to_owned());
            }
        }
        let source = fs::read_to_string(&file).map_err(|e| e.to_string())?;
        let parsed =
            syn::parse_file(&source).map_err(|error| format!("{}: {error}", file.display()))?;
        let directory = if matches!(
            file.file_name().and_then(|n| n.to_str()),
            Some("lib.rs" | "mod.rs")
        ) {
            file.parent().unwrap().to_owned()
        } else {
            file.with_extension("")
        };
        self.load_items(name, owner, &file, &directory, parsed.items, root, stack)?;
        stack.remove(&file);
        Ok(())
    }

    fn load_items(
        &mut self,
        name: Name,
        owner: Name,
        file: &Path,
        directory: &Path,
        items: Vec<Item>,
        root: &Path,
        stack: &mut BTreeSet<PathBuf>,
    ) -> Result<(), String> {
        for item in &items {
            let Item::Mod(module) = item else {
                continue;
            };
            if test_only(&module.attrs) {
                continue;
            }
            if module
                .attrs
                .iter()
                .any(|attr| attr.path().is_ident("cfg_attr"))
            {
                return Err(format!(
                    "{}:{}: cfg_attr module ownership is unsupported; use an explicit module path",
                    file.display(),
                    module.span().start().line
                ));
            }
            let id = identifier(&module.ident);
            let mut child = name.clone();
            child.push(id.clone());
            if let Some((_, contents)) = &module.content {
                if module.attrs.iter().any(|attr| attr.path().is_ident("path")) {
                    return Err(format!(
                        "{}:{}: inline #[path] ownership is unsupported; use a named external module",
                        file.display(),
                        module.span().start().line
                    ));
                }
                let mut child_owner = owner.clone();
                child_owner.push(id.clone());
                self.load_items(
                    child,
                    child_owner,
                    file,
                    &directory.join(id),
                    contents.clone(),
                    root,
                    stack,
                )?;
            } else {
                let explicit = module
                    .attrs
                    .iter()
                    .find(|attr| attr.path().is_ident("path"))
                    .map(|attr| {
                        let syn::Meta::NameValue(value) = &attr.meta else {
                            return Err("Expected literal #[path = \"...\"]".to_owned());
                        };
                        let syn::Expr::Lit(value) = &value.value else {
                            return Err("Module path must be a literal".to_owned());
                        };
                        let syn::Lit::Str(value) = &value.lit else {
                            return Err("Module path must be a string".to_owned());
                        };
                        Ok(file.parent().unwrap().join(value.value()))
                    })
                    .transpose()?;
                let relocated = explicit.is_some();
                let target = explicit.unwrap_or_else(|| {
                    let flat = directory.join(format!("{id}.rs"));
                    if flat.exists() {
                        flat
                    } else {
                        directory.join(id).join("mod.rs")
                    }
                });
                self.load_file(child.clone(), &target, root, stack)?;
                if relocated {
                    let target = &self.modules[&child].owner;
                    if let Some(rule) = rules::module_edge(&owner, target) {
                        self.issues.insert(format!(
                            "{}:{}: #[path] {} -> {}: {rule}",
                            file.display(),
                            module.span().start().line,
                            owner.join("::"),
                            target.join("::")
                        ));
                    }
                }
            }
        }
        let imports = Imports::collect(items.iter().cloned());
        let symbols = items
            .iter()
            .filter_map(|item| match item {
                Item::Fn(i) => Some(&i.sig.ident),
                Item::Struct(i) => Some(&i.ident),
                Item::Enum(i) => Some(&i.ident),
                Item::Trait(i) => Some(&i.ident),
                Item::Const(i) => Some(&i.ident),
                Item::Static(i) => Some(&i.ident),
                Item::Type(i) => Some(&i.ident),
                Item::Mod(i) => Some(&i.ident),
                Item::Union(i) => Some(&i.ident),
                _ => None,
            })
            .map(identifier)
            .collect();
        self.modules.insert(
            name,
            Module {
                file: file.to_owned(),
                owner,
                items,
                imports,
                symbols,
            },
        );
        Ok(())
    }

    fn exists(&self, target: &[String]) -> bool {
        if !self.modules.contains_key(&target[..1]) {
            return true;
        }
        for index in (1..=target.len()).rev() {
            if let Some(module) = self.modules.get(&target[..index]) {
                return index == target.len() || module.symbols.contains(&target[index]);
            }
        }
        false
    }

    /// The resolver follows imports at each path segment, including root-level facades.
    fn resolve(
        &self,
        context: &[String],
        raw: &[String],
        local: &[Imports],
        seen: &mut BTreeSet<(Name, Name)>,
    ) -> Vec<Name> {
        if raw.is_empty() || !seen.insert((context.to_vec(), raw.to_vec())) {
            return vec![];
        }
        let mut absolute = match raw[0].as_str() {
            "crate" => [context[..1].to_vec(), raw[1..].to_vec()].concat(),
            "self" => [context.to_vec(), raw[1..].to_vec()].concat(),
            "super" => {
                let count = raw
                    .iter()
                    .take_while(|part| part.as_str() == "super")
                    .count();
                if count >= context.len() {
                    return vec![];
                }
                [
                    context[..context.len() - count].to_vec(),
                    raw[count..].to_vec(),
                ]
                .concat()
            }
            first => {
                for bindings in local.iter().rev() {
                    if let Some(alias) = bindings.aliases.get(first) {
                        return self.resolve(
                            context,
                            &[alias.clone(), raw[1..].to_vec()].concat(),
                            local,
                            seen,
                        );
                    }
                }
                if let Some(alias) = self
                    .modules
                    .get(context)
                    .and_then(|module| module.imports.aliases.get(first))
                {
                    return self.resolve(
                        context,
                        &[alias.clone(), raw[1..].to_vec()].concat(),
                        &[],
                        seen,
                    );
                }
                if let Some(package) = self
                    .dependencies
                    .get(&context[0])
                    .and_then(|deps| deps.get(first))
                {
                    [vec![package.clone()], raw[1..].to_vec()].concat()
                } else if self.modules.contains_key(&vec![first.to_owned()]) {
                    raw.to_vec()
                } else {
                    [context.to_vec(), raw.to_vec()].concat()
                }
            }
        };
        for index in 1..absolute.len() {
            let prefix = &absolute[..index];
            let Some(module) = self.modules.get(prefix) else {
                break;
            };
            let next = &absolute[index];
            if let Some(alias) = module.imports.aliases.get(next) {
                let mut redirected = alias.clone();
                redirected.extend_from_slice(&absolute[index + 1..]);
                return self.resolve(prefix, &redirected, &[], seen);
            }
            if !self.modules.contains_key(&absolute[..index + 1])
                && !module.imports.globs.is_empty()
            {
                let mut targets = vec![];
                for glob in &module.imports.globs {
                    let candidate = [glob.clone(), absolute[index..].to_vec()].concat();
                    targets.extend(
                        self.resolve(prefix, &candidate, &[], &mut seen.clone())
                            .into_iter()
                            .filter(|target| self.exists(target)),
                    );
                }
                if !targets.is_empty() {
                    return targets;
                }
            }
        }
        for index in (1..=absolute.len()).rev() {
            if let Some(module) = self.modules.get(&absolute[..index]) {
                absolute = [module.owner.clone(), absolute[index..].to_vec()].concat();
                break;
            }
        }
        vec![absolute]
    }

    pub fn violations(&self) -> BTreeSet<String> {
        let mut failures = self.issues.clone();
        for (name, module) in &self.modules {
            if !rules::checked_module(&module.owner) {
                continue;
            }
            let mut visitor = References {
                sources: self,
                name,
                module,
                scopes: vec![],
                failures: &mut failures,
                includes: BTreeSet::new(),
                include_files: vec![],
            };
            for item in &module.items {
                visitor.visit_item(item);
            }
        }
        failures
    }
}

struct References<'a> {
    sources: &'a Sources,
    name: &'a [String],
    module: &'a Module,
    scopes: Vec<Imports>,
    failures: &'a mut BTreeSet<String>,
    includes: BTreeSet<PathBuf>,
    include_files: Vec<PathBuf>,
}

impl References<'_> {
    fn check(&mut self, path: Name, span: Span) {
        for target in self
            .sources
            .resolve(self.name, &path, &self.scopes, &mut BTreeSet::new())
        {
            if let Some(rule) = rules::module_edge(&self.module.owner, &target) {
                self.failures.insert(format!(
                    "{}:{}: {} -> {}: {rule}",
                    self.module.file.display(),
                    span.start().line,
                    self.module.owner.join("::"),
                    target.join("::")
                ));
            }
        }
    }

    /// Macro token groups retain identifiers but omit comments and string contents.
    fn tokens(&mut self, tokens: TokenStream) {
        let tokens: Vec<_> = tokens.into_iter().collect();
        for token in &tokens {
            if let TokenTree::Group(group) = token {
                self.tokens(group.stream());
            }
            if let TokenTree::Ident(ident) = token {
                if matches!(ident.to_string().as_str(), "mod" | "use" | "extern") {
                    self.failures.insert(format!("{}:{}: macro-generated module/import ownership is unsupported; use explicit Rust declarations", self.module.file.display(), ident.span().start().line));
                }
            }
        }
        let mut index = 0;
        while index < tokens.len() {
            let TokenTree::Ident(first) = &tokens[index] else {
                index += 1;
                continue;
            };
            let start = first.span();
            let mut parts = vec![identifier(first)];
            let mut end = index + 1;
            while end + 2 < tokens.len() {
                if !matches!((&tokens[end], &tokens[end+1]), (TokenTree::Punct(a),TokenTree::Punct(b)) if a.as_char()==':' && b.as_char()==':')
                {
                    break;
                }
                let TokenTree::Ident(next) = &tokens[end + 2] else {
                    break;
                };
                parts.push(identifier(next));
                end += 3;
            }
            if parts.len() > 1 {
                self.check(parts, start);
            }
            index = end;
        }
    }
}

impl<'ast> Visit<'ast> for References<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        let attrs = match item {
            Item::Fn(item) => &item.attrs,
            Item::Mod(item) => &item.attrs,
            Item::Use(item) => &item.attrs,
            Item::Impl(item) => &item.attrs,
            Item::Const(item) => &item.attrs,
            Item::Type(item) => &item.attrs,
            _ => &[][..],
        };
        if !test_only(attrs) {
            visit::visit_item(self, item);
        }
    }
    fn visit_item_mod(&mut self, _: &'ast syn::ItemMod) {}
    fn visit_attribute(&mut self, _: &'ast syn::Attribute) {}
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let mut paths = vec![];
        use_paths(&item.tree, vec![], &mut paths);
        for (_, target) in paths {
            self.check(target, item.span());
        }
    }
    fn visit_path(&mut self, item: &'ast syn::Path) {
        self.check(path(item), item.span());
        visit::visit_path(self, item);
    }
    fn visit_block(&mut self, block: &'ast syn::Block) {
        let items = block.stmts.iter().filter_map(|statement| {
            if let syn::Stmt::Item(item) = statement {
                Some(item.clone())
            } else {
                None
            }
        });
        self.scopes.push(Imports::collect(items));
        visit::visit_block(self, block);
        self.scopes.pop();
    }
    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        self.check(path(&item.path), item.span());
        if item.path.is_ident("include") {
            let result = (|| -> Result<(PathBuf, syn::Expr), String> {
                let literal = syn::parse2::<syn::LitStr>(item.tokens.clone()).map_err(|_| {
                    "include! requires a literal source path for ownership checking".to_owned()
                })?;
                let file = self
                    .include_files
                    .last()
                    .unwrap_or(&self.module.file)
                    .parent()
                    .unwrap()
                    .join(literal.value())
                    .canonicalize()
                    .map_err(|error| error.to_string())?;
                if self.includes.contains(&file) {
                    return Err("recursive include! source ownership".into());
                }
                let source = fs::read_to_string(&file).map_err(|error| error.to_string())?;
                let expression = syn::parse_str::<syn::Expr>(&source).map_err(|_| {
                    "item-generating include! is unsupported; use an explicit Rust module"
                        .to_owned()
                })?;
                Ok((file, expression))
            })();
            match result {
                Ok((file, expression)) => {
                    self.includes.insert(file.clone());
                    self.include_files.push(file.clone());
                    self.visit_expr(&expression);
                    self.include_files.pop();
                    self.includes.remove(&file);
                }
                Err(error) => {
                    self.failures.insert(format!(
                        "{}:{}: {error}",
                        self.module.file.display(),
                        item.span().start().line
                    ));
                }
            }
        } else {
            self.tokens(item.tokens.clone());
        }
    }
}

#[cfg(test)]
mod tests;
