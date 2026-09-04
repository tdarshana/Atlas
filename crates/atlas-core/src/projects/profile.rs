use std::path::Path;
use crate::models::ProjectProfile;
use crate::Result;

const MANIFESTS: &[(&str, &str)] = &[
    ("package.json", "javascript"),
    ("Cargo.toml", "rust"),
    ("pyproject.toml", "python"),
    ("requirements.txt", "python"),
    ("go.mod", "go"),
    ("Gemfile", "ruby"),
    ("pom.xml", "java"),
    ("build.gradle", "java"),
];
const JS_FRAMEWORKS: &[&str] = &[
    "next", "react", "svelte", "@sveltejs/kit", "vue", "nuxt", "astro", "express", "fastify",
    "hono", "convex", "tailwindcss", "@tauri-apps/api", "expo", "react-native",
];
const RUST_FRAMEWORKS: &[&str] = &["tauri", "axum", "actix-web", "tokio", "rocket", "bevy", "ratatui"];
const PY_FRAMEWORKS: &[&str] = &["django", "fastapi", "flask", "langchain", "torch", "numpy"];

/// Builds a heuristic profile of the project at `root`: languages/frameworks
/// detected from manifest files, a two-level directory tree (git-ignored
/// paths excluded), the README head, and recent commit subjects.
pub fn build_profile(root: &Path) -> Result<ProjectProfile> {
    let mut p = ProjectProfile {
        name: root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".into()),
        ..Default::default()
    };
    for (file, lang) in MANIFESTS {
        if root.join(file).exists() && !p.languages.contains(&lang.to_string()) {
            p.languages.push(lang.to_string());
        }
    }
    if let Ok(pkg) = std::fs::read_to_string(root.join("package.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&pkg) {
            if let Some(n) = v["name"].as_str() {
                p.name = n.to_string();
            }
            let deps: Vec<String> = ["dependencies", "devDependencies"]
                .iter()
                .flat_map(|k| v[k].as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default())
                .collect();
            if deps.iter().any(|d| d == "typescript") || root.join("tsconfig.json").exists() {
                p.languages.push("typescript".into());
            }
            for f in JS_FRAMEWORKS {
                if deps.iter().any(|d| d == f) {
                    p.frameworks.push(f.trim_start_matches('@').to_string());
                }
            }
        }
    }
    if let Ok(cargo) = std::fs::read_to_string(root.join("Cargo.toml")) {
        for f in RUST_FRAMEWORKS {
            if cargo.contains(&format!("\n{f} ")) || cargo.contains(&format!("\n{f}=")) || cargo.contains(&format!("\n{f}.")) {
                p.frameworks.push(f.to_string());
            }
        }
    }
    if let Ok(py) = std::fs::read_to_string(root.join("pyproject.toml")) {
        for f in PY_FRAMEWORKS {
            if py.contains(f) {
                p.frameworks.push(f.to_string());
            }
        }
    }
    p.frameworks.sort();
    p.frameworks.dedup();
    p.tree = tree_two_levels(root);
    // Manifest/tsconfig checks above miss a bare `.ts`/`.tsx` source tree with no
    // tsconfig.json and no `typescript` devDependency; fall back to the tree scan.
    if p.tree.iter().any(|t| t.ends_with(".ts") || t.ends_with(".tsx")) && !p.languages.contains(&"typescript".to_string()) {
        p.languages.push("typescript".into());
    }
    for candidate in ["README.md", "readme.md", "README"] {
        if let Ok(s) = std::fs::read_to_string(root.join(candidate)) {
            p.readme_head = s.lines().take(60).collect::<Vec<_>>().join("\n");
            break;
        }
    }
    p.recent_commits = recent_commits(root, 10);
    p.planning_frameworks = crate::frameworks::detect_all(root);
    p.built_at = chrono::Utc::now();
    Ok(p)
}

fn tree_two_levels(root: &Path) -> Vec<String> {
    let mut out = vec![];
    let walker = ignore::WalkBuilder::new(root)
        .max_depth(Some(2))
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .build();
    for e in walker.flatten() {
        if e.depth() == 0 {
            continue;
        }
        if let Ok(rel) = e.path().strip_prefix(root) {
            let mut s = rel.to_string_lossy().to_string();
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                s.push('/');
            }
            out.push(s);
        }
        if out.len() >= 200 {
            break;
        }
    }
    out.sort();
    out
}

fn recent_commits(root: &Path, n: usize) -> Vec<String> {
    let Ok(repo) = git2::Repository::discover(root) else { return vec![] };
    let Ok(mut walk) = repo.revwalk() else { return vec![] };
    if walk.push_head().is_err() {
        return vec![];
    }
    walk.flatten()
        .take(n)
        .filter_map(|oid| repo.find_commit(oid).ok())
        .map(|c| c.summary().ok().flatten().unwrap_or("").to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_common as common;

    #[test]
    fn profile_detects_stack_tree_readme_commits() {
        let d = tempfile::tempdir().unwrap();
        common::fixture_repo(d.path());
        let p = build_profile(d.path()).unwrap();
        assert_eq!(p.name, "fixture");
        assert!(p.languages.contains(&"typescript".to_string()));
        assert!(p.frameworks.contains(&"next".to_string()) && p.frameworks.contains(&"react".to_string()));
        assert!(p.tree.iter().any(|t| t == "src/index.ts"));
        assert!(!p.tree.iter().any(|t| t.starts_with("node_modules")), "gitignored dirs excluded");
        assert!(p.readme_head.starts_with("# fixture"));
        assert_eq!(p.recent_commits, vec!["initial fixture".to_string()]);
    }
}
