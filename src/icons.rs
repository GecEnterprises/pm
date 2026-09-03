//! File-type icons for the Explorer tree. The SVGs and the extension→icon
//! mapping tables are ported from Zed (`assets/icons/file_icons/` and
//! `crates/theme/src/icon_theme.rs`). gpui's `paint_svg` is monochrome, so every
//! icon is a single-path glyph tinted at paint time.

use std::path::Path;

use include_dir::{include_dir, Dir};

static ICONS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets/file_icons");

fn bytes(name: &str) -> &'static [u8] {
    ICONS
        .get_file(name)
        .or_else(|| ICONS.get_file("file.svg"))
        .expect("file.svg present")
        .contents()
}

/// Full filenames / stems that map directly to an icon key.
const FILE_STEMS_BY_ICON_KEY: &[(&str, &[&str])] = &[
    ("docker", &["Containerfile", "Dockerfile", ".dockerignore"]),
    ("ruby", &["Podfile"]),
    ("heroku", &["Procfile"]),
];

/// File suffixes (and some full filenames) that map to an icon key.
const FILE_SUFFIXES_BY_ICON_KEY: &[(&str, &[&str])] = &[
    ("astro", &["astro"]),
    ("audio", &["aac", "flac", "m4a", "mka", "mp3", "ogg", "opus", "wav", "wma", "wv"]),
    ("backup", &["bak"]),
    ("ballerina", &["bal"]),
    ("bicep", &["bicep"]),
    ("bun", &["lockb"]),
    ("c", &["c", "h"]),
    ("cairo", &["cairo"]),
    ("code", &["handlebars", "metadata", "rkt", "scm"]),
    ("coffeescript", &["coffee"]),
    ("cpp", &["c++", "h++", "cc", "cpp", "cppm", "cxx", "hh", "hpp", "hxx", "inl", "ixx"]),
    ("crystal", &["cr", "ecr"]),
    ("csharp", &["cs"]),
    ("csproj", &["csproj"]),
    ("css", &["css", "pcss", "postcss"]),
    ("cue", &["cue"]),
    ("dart", &["dart"]),
    ("diff", &["diff"]),
    ("docker", &["docker-compose.yml", "docker-compose.yaml", "compose.yml", "compose.yaml"]),
    ("document", &[
        "doc", "docx", "mdx", "odp", "ods", "odt", "pdf", "ppt", "pptx", "rtf", "txt", "xls", "xlsx",
    ]),
    ("editorconfig", &["editorconfig"]),
    ("elixir", &["eex", "ex", "exs", "heex", "leex", "neex"]),
    ("elm", &["elm"]),
    ("erlang", &["Emakefile", "app.src", "erl", "escript", "hrl", "rebar.config", "xrl", "yrl"]),
    ("eslint", &[
        "eslint.config.cjs", "eslint.config.cts", "eslint.config.js", "eslint.config.mjs",
        "eslint.config.mts", "eslint.config.ts", "eslintrc", "eslintrc.js", "eslintrc.json",
    ]),
    ("font", &["otf", "ttf", "woff", "woff2"]),
    ("fsharp", &["fs"]),
    ("fsproj", &["fsproj"]),
    ("gitlab", &["gitlab-ci.yml", "gitlab-ci.yaml"]),
    ("gleam", &["gleam"]),
    ("go", &["go", "mod", "work"]),
    ("graphql", &["gql", "graphql", "graphqls"]),
    ("haskell", &["hs"]),
    ("hcl", &["hcl"]),
    ("helm", &[
        "helmfile.yaml", "helmfile.yml", "Chart.yaml", "Chart.yml", "Chart.lock", "values.yaml",
        "values.yml", "requirements.yaml", "requirements.yml", "tpl",
    ]),
    ("html", &["htm", "html"]),
    ("image", &[
        "avif", "bmp", "gif", "heic", "heif", "ico", "j2k", "jfif", "jp2", "jpeg", "jpg", "jxl",
        "png", "psd", "qoi", "svg", "tiff", "webp",
    ]),
    ("ipynb", &["ipynb"]),
    ("java", &["java"]),
    ("javascript", &["cjs", "js", "mjs"]),
    ("json", &["json", "jsonc"]),
    ("julia", &["jl"]),
    ("kdl", &["kdl"]),
    ("kotlin", &["kt"]),
    ("lock", &["lock"]),
    ("log", &["log"]),
    ("lua", &["lua"]),
    ("luau", &["luau"]),
    ("markdown", &["markdown", "md"]),
    ("metal", &["metal"]),
    ("nim", &["nim", "nims", "nimble"]),
    ("nix", &["nix"]),
    ("ocaml", &["ml", "mli", "mlx"]),
    ("odin", &["odin"]),
    ("php", &["php"]),
    ("prettier", &[
        "prettier.config.cjs", "prettier.config.js", "prettier.config.mjs", "prettierignore",
        "prettierrc", "prettierrc.cjs", "prettierrc.js", "prettierrc.json", "prettierrc.json5",
        "prettierrc.mjs", "prettierrc.toml", "prettierrc.yaml", "prettierrc.yml",
    ]),
    ("prisma", &["prisma"]),
    ("puppet", &["pp"]),
    ("python", &["py"]),
    ("r", &["r", "R"]),
    ("react", &["cjsx", "ctsx", "jsx", "mjsx", "mtsx", "tsx"]),
    ("roc", &["roc"]),
    ("ruby", &["rb"]),
    ("rust", &["rs"]),
    ("sass", &["sass", "scss"]),
    ("scala", &["scala", "sc"]),
    ("settings", &["conf", "ini"]),
    ("solidity", &["sol"]),
    ("storage", &[
        "accdb", "csv", "dat", "db", "dbf", "dll", "fmp", "fp7", "frm", "gdb", "ib", "ldf", "mdb",
        "mdf", "myd", "myi", "pdb", "psv", "RData", "rdata", "sav", "sdf", "sql", "sqlite", "ssv",
        "tsv",
    ]),
    ("stylelint", &[
        "stylelint.config.cjs", "stylelint.config.js", "stylelint.config.mjs", "stylelintignore",
        "stylelintrc", "stylelintrc.cjs", "stylelintrc.js", "stylelintrc.json", "stylelintrc.mjs",
        "stylelintrc.yaml", "stylelintrc.yml",
    ]),
    ("surrealql", &["surql"]),
    ("svelte", &["svelte"]),
    ("swift", &["swift"]),
    ("tcl", &["tcl"]),
    ("template", &["hbs", "plist", "xml"]),
    ("terminal", &[
        "bash", "bash_aliases", "bash_login", "bash_logout", "bash_profile", "bashrc", "brushrc",
        "fish", "nu", "profile", "ps1", "sh", "zlogin", "zlogout", "zprofile", "zsh", "zsh_aliases",
        "zsh_histfile", "zsh_history", "zshenv", "zshrc",
    ]),
    ("terraform", &["tf", "tfvars"]),
    ("toml", &["toml"]),
    ("typescript", &["cts", "mts", "ts"]),
    ("v", &["v", "vsh", "vv"]),
    ("vcs", &[
        "COMMIT_EDITMSG", "EDIT_DESCRIPTION", "MERGE_MSG", "NOTES_EDITMSG", "TAG_EDITMSG",
        "gitattributes", "gitignore", "gitkeep", "gitmodules",
    ]),
    ("vbproj", &["vbproj"]),
    ("video", &["avi", "m4v", "mkv", "mov", "mp4", "webm", "wmv"]),
    ("vs_sln", &["sln"]),
    ("vs_suo", &["suo"]),
    ("vue", &["vue"]),
    ("vyper", &["vy", "vyi"]),
    ("wgsl", &["wgsl"]),
    ("yaml", &["yaml", "yml"]),
    ("zig", &["zig"]),
];

/// Icon key → bundled SVG filename. Keys with no dedicated glyph fall back to `file.svg`.
const FILE_ICONS: &[(&str, &str)] = &[
    ("astro", "astro.svg"),
    ("audio", "audio.svg"),
    ("ballerina", "ballerina.svg"),
    ("bun", "bun.svg"),
    ("c", "c.svg"),
    ("cairo", "cairo.svg"),
    ("code", "code.svg"),
    ("coffeescript", "coffeescript.svg"),
    ("cpp", "cpp.svg"),
    ("css", "css.svg"),
    ("dart", "dart.svg"),
    ("default", "file.svg"),
    ("diff", "diff.svg"),
    ("docker", "docker.svg"),
    ("document", "book.svg"),
    ("editorconfig", "editorconfig.svg"),
    ("elixir", "elixir.svg"),
    ("elm", "elm.svg"),
    ("erlang", "erlang.svg"),
    ("eslint", "eslint.svg"),
    ("font", "font.svg"),
    ("fsharp", "fsharp.svg"),
    ("gitlab", "gitlab.svg"),
    ("gleam", "gleam.svg"),
    ("go", "go.svg"),
    ("graphql", "graphql.svg"),
    ("haskell", "haskell.svg"),
    ("hcl", "hcl.svg"),
    ("helm", "helm.svg"),
    ("heroku", "heroku.svg"),
    ("html", "html.svg"),
    ("image", "image.svg"),
    ("ipynb", "jupyter.svg"),
    ("java", "java.svg"),
    ("javascript", "javascript.svg"),
    ("json", "code.svg"),
    ("julia", "julia.svg"),
    ("kdl", "kdl.svg"),
    ("kotlin", "kotlin.svg"),
    ("lock", "lock.svg"),
    ("log", "info.svg"),
    ("lua", "lua.svg"),
    ("luau", "luau.svg"),
    ("markdown", "book.svg"),
    ("metal", "metal.svg"),
    ("nim", "nim.svg"),
    ("nix", "nix.svg"),
    ("ocaml", "ocaml.svg"),
    ("odin", "odin.svg"),
    ("phoenix", "phoenix.svg"),
    ("php", "php.svg"),
    ("prettier", "prettier.svg"),
    ("prisma", "prisma.svg"),
    ("puppet", "puppet.svg"),
    ("python", "python.svg"),
    ("r", "r.svg"),
    ("react", "react.svg"),
    ("roc", "roc.svg"),
    ("ruby", "ruby.svg"),
    ("rust", "rust.svg"),
    ("sass", "sass.svg"),
    ("scala", "scala.svg"),
    ("settings", "settings.svg"),
    ("storage", "database.svg"),
    ("stylelint", "javascript.svg"),
    ("surrealql", "surrealql.svg"),
    ("svelte", "html.svg"),
    ("swift", "swift.svg"),
    ("tcl", "tcl.svg"),
    ("template", "html.svg"),
    ("terminal", "terminal.svg"),
    ("terraform", "terraform.svg"),
    ("toml", "toml.svg"),
    ("typescript", "typescript.svg"),
    ("v", "v.svg"),
    ("vcs", "git.svg"),
    ("video", "video.svg"),
    ("vue", "vue.svg"),
    ("vyper", "vyper.svg"),
    ("wgsl", "wgsl.svg"),
    ("yaml", "yaml.svg"),
    ("zig", "zig.svg"),
];

fn lookup_key(s: &str) -> Option<&'static str> {
    FILE_STEMS_BY_ICON_KEY
        .iter()
        .chain(FILE_SUFFIXES_BY_ICON_KEY)
        .find_map(|(key, names)| names.contains(&s).then_some(*key))
}

fn svg_for_key(key: &str) -> &'static str {
    FILE_ICONS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, f)| *f)
        .unwrap_or("file.svg")
}

/// `(stable paint_svg cache key = bundled filename, raw SVG bytes)`.
///
/// Lookup ladder (from Zed's `FileIcons::get_icon`): full filename → progressive
/// `.`-suffixes → extension (or dot-stripped name) → `"default"`.
pub fn icon_bytes_for(path: &Path, is_dir: bool, expanded: bool) -> (&'static str, &'static [u8]) {
    if is_dir {
        let f = if expanded { "folder_open.svg" } else { "folder.svg" };
        return (f, bytes(f));
    }

    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or_default();
    let mut key = lookup_key(name);

    if key.is_none() {
        let mut rest = name;
        while let Some((_, after)) = rest.split_once('.') {
            if let Some(k) = lookup_key(after) {
                key = Some(k);
                break;
            }
            rest = after;
        }
    }

    if key.is_none() {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .or_else(|| name.strip_prefix('.'));
        if let Some(e) = ext {
            key = lookup_key(e);
        }
    }

    let file = svg_for_key(key.unwrap_or("default"));
    (file, bytes(file))
}

pub fn chevron_bytes(expanded: bool) -> (&'static str, &'static [u8]) {
    let f = if expanded {
        "chevron_down.svg"
    } else {
        "chevron_right.svg"
    };
    (f, bytes(f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn maps_known_types() {
        assert_eq!(icon_bytes_for(Path::new("src/main.rs"), false, false).0, "rust.svg");
        assert_eq!(icon_bytes_for(Path::new("Cargo.toml"), false, false).0, "toml.svg");
        assert_eq!(icon_bytes_for(Path::new("readme.md"), false, false).0, "book.svg");
        assert_eq!(icon_bytes_for(Path::new(".gitignore"), false, false).0, "git.svg");
        assert_eq!(icon_bytes_for(Path::new("docker-compose.yml"), false, false).0, "docker.svg");
    }

    #[test]
    fn unknown_falls_back_to_file() {
        assert_eq!(icon_bytes_for(Path::new("weird.xyzzy"), false, false).0, "file.svg");
        assert_eq!(icon_bytes_for(Path::new("NOEXT"), false, false).0, "file.svg");
    }

    #[test]
    fn directories() {
        assert_eq!(icon_bytes_for(Path::new("src"), true, false).0, "folder.svg");
        assert_eq!(icon_bytes_for(Path::new("src"), true, true).0, "folder_open.svg");
    }
}
