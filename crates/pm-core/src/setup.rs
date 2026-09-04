//! `pm --setup` / `pm --uninstall` — Windows registration and MCP client
//! wiring (PM-5).
//!
//! `--setup` is what `install.ps1` runs after dropping `pm.exe` in place. It:
//!   - makes sure the install dir is on the user PATH,
//!   - writes the `App Paths` and `Uninstall` registry keys (so pm shows up in
//!     Windows "Installed apps" / BCUninstaller with a working uninstaller),
//!   - optionally adds a Start Menu shortcut,
//!   - registers pm's MCP server with Claude Code.
//!
//! Everything is best-effort and idempotent — re-running it just rewrites the
//! same state. It shells out to `reg.exe` / `powershell` rather than pulling in
//! a registry crate.

use anyhow::Result;

#[cfg(windows)]
pub fn run(assume_yes: bool) -> Result<()> {
    imp::run(assume_yes)
}

#[cfg(windows)]
pub fn uninstall(assume_yes: bool) -> Result<()> {
    imp::uninstall(assume_yes)
}

#[cfg(not(windows))]
pub fn run(_assume_yes: bool) -> Result<()> {
    anyhow::bail!("`pm --setup` is Windows-only for now")
}

#[cfg(not(windows))]
pub fn uninstall(_assume_yes: bool) -> Result<()> {
    anyhow::bail!("`pm --uninstall` is Windows-only for now")
}

#[cfg(windows)]
mod imp {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use anyhow::{anyhow, Context, Result};

    use crate::buildinfo as build;

    const REPO_URL: &str = "https://github.com/GecEnterprises/pm";
    const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\pm";
    const APP_PATHS_KEY: &str =
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths\pm.exe";

    pub fn run(assume_yes: bool) -> Result<()> {
        let exe = std::env::current_exe().context("locating pm.exe")?;
        let dir = exe
            .parent()
            .context("pm.exe has no parent directory")?
            .to_path_buf();

        banner();
        println!("setup — {}\n", exe.display());

        ensure_on_path(&dir)?;
        register_app_paths(&exe, &dir)?;
        register_uninstaller(&exe, &dir)?;
        println!("  registry: App Paths + uninstall entry written");

        if confirm("Add pm to the Start Menu?", true, assume_yes) {
            match create_start_menu_shortcut(&exe) {
                Ok(p) => println!("  Start Menu: {}", p.display()),
                Err(e) => eprintln!("  Start Menu shortcut failed: {e}"),
            }
        }

        if confirm("Register pm's MCP server with Claude Code?", true, assume_yes) {
            match mcp_plug() {
                Ok(how) => println!("  Claude Code MCP: {how}"),
                Err(e) => eprintln!("  Claude Code MCP wiring failed: {e}"),
            }
        }

        println!("Done. Open a new terminal for the PATH change to take effect.");
        Ok(())
    }

    pub fn uninstall(assume_yes: bool) -> Result<()> {
        if !confirm("Remove pm and undo its registration?", true, assume_yes) {
            println!("Cancelled.");
            return Ok(());
        }
        let exe = std::env::current_exe().context("locating pm.exe")?;
        let dir = exe.parent().map(Path::to_path_buf);

        best_effort("Claude Code MCP entry", mcp_unplug());
        best_effort("uninstall registry key", reg_delete(UNINSTALL_KEY));
        best_effort("App Paths registry key", reg_delete(APP_PATHS_KEY));
        best_effort("Start Menu shortcut", remove_start_menu_shortcut());
        if let Some(dir) = &dir {
            best_effort("PATH entry", remove_from_path(dir));
        }
        best_effort("pm.exe", schedule_self_delete(&exe));

        println!("pm uninstalled.");
        Ok(())
    }

    // ---- banner + prompting ----------------------------------------------

    fn banner() {
        match figlet_rs::FIGfont::standard() {
            Ok(font) => match font.convert("pm") {
                Some(art) => println!("\n{art}"),
                None => println!("\n== pm =="),
            },
            Err(_) => println!("\n== pm =="),
        }
    }

    /// A yes/no prompt via `dialoguer`. `--yes` (or a non-interactive stdin)
    /// takes the default without asking.
    fn confirm(question: &str, default_yes: bool, assume_yes: bool) -> bool {
        use std::io::IsTerminal;
        if assume_yes || !std::io::stdin().is_terminal() {
            return default_yes;
        }
        dialoguer::Confirm::new()
            .with_prompt(question)
            .default(default_yes)
            .interact()
            .unwrap_or(default_yes)
    }

    fn best_effort(what: &str, r: Result<()>) {
        match r {
            Ok(()) => println!("  removed {what}"),
            Err(e) => eprintln!("  could not remove {what}: {e}"),
        }
    }

    // ---- registry ---------------------------------------------------------

    fn reg(args: &[&str]) -> Result<()> {
        let out = Command::new("reg")
            .args(args)
            .output()
            .context("running reg.exe")?;
        if !out.status.success() {
            return Err(anyhow!(
                "reg {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }

    fn reg_set(key: &str, name: Option<&str>, ty: &str, data: &str) -> Result<()> {
        let mut a: Vec<&str> = vec!["add", key];
        match name {
            Some(n) => a.extend(["/v", n]),
            None => a.push("/ve"),
        }
        a.extend(["/t", ty, "/d", data, "/f"]);
        reg(&a)
    }

    fn reg_delete(key: &str) -> Result<()> {
        reg(&["delete", key, "/f"])
    }

    fn register_app_paths(exe: &Path, dir: &Path) -> Result<()> {
        let exe = exe.to_string_lossy();
        let dir = dir.to_string_lossy();
        reg_set(APP_PATHS_KEY, None, "REG_SZ", &exe)?;
        reg_set(APP_PATHS_KEY, Some("Path"), "REG_SZ", &dir)?;
        Ok(())
    }

    fn register_uninstaller(exe: &Path, dir: &Path) -> Result<()> {
        let exe_s = exe.to_string_lossy().to_string();
        let dir_s = dir.to_string_lossy().to_string();
        let uninstall = format!("\"{exe_s}\" --uninstall --yes");
        let size_kib = std::fs::metadata(exe)
            .map(|m| m.len() / 1024)
            .unwrap_or(0)
            .to_string();

        let pairs: Vec<(&str, &str, &str)> = vec![
            ("DisplayName", "REG_SZ", "pm (Plus Minus)"),
            ("DisplayVersion", "REG_SZ", build::VERSION),
            ("DisplayIcon", "REG_SZ", &exe_s),
            ("Publisher", "REG_SZ", "GecEnterprises"),
            ("InstallLocation", "REG_SZ", &dir_s),
            ("UninstallString", "REG_SZ", &uninstall),
            ("QuietUninstallString", "REG_SZ", &uninstall),
            ("URLInfoAbout", "REG_SZ", REPO_URL),
            ("EstimatedSize", "REG_DWORD", &size_kib),
            ("NoModify", "REG_DWORD", "1"),
            ("NoRepair", "REG_DWORD", "1"),
        ];
        for (name, ty, data) in pairs {
            reg_set(UNINSTALL_KEY, Some(name), ty, data)?;
        }
        Ok(())
    }

    // ---- PATH (via PowerShell, like install.ps1) -------------------------

    fn ps(script: &str) -> Result<String> {
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .context("running powershell")?;
        if !out.status.success() {
            return Err(anyhow!(
                "powershell: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Single-quote a string for embedding in a PowerShell literal.
    fn psq(s: &str) -> String {
        s.replace('\'', "''")
    }

    fn ensure_on_path(dir: &Path) -> Result<()> {
        let d = psq(&dir.to_string_lossy());
        let script = format!(
            "$d='{d}'; $p=[Environment]::GetEnvironmentVariable('Path','User'); \
             if (($p -split ';') -notcontains $d) {{ \
             [Environment]::SetEnvironmentVariable('Path', ($p.TrimEnd(';') + ';' + $d), 'User'); \
             'added' }} else {{ 'present' }}"
        );
        match ps(&script)?.as_str() {
            "added" => println!("  PATH: added {}", dir.display()),
            _ => println!("  PATH: already present"),
        }
        Ok(())
    }

    fn remove_from_path(dir: &Path) -> Result<()> {
        let d = psq(&dir.to_string_lossy());
        let script = format!(
            "$d='{d}'; $p=[Environment]::GetEnvironmentVariable('Path','User'); \
             $n=(($p -split ';') | Where-Object {{ $_ -and $_ -ne $d }}) -join ';'; \
             [Environment]::SetEnvironmentVariable('Path', $n, 'User')"
        );
        ps(&script).map(|_| ())
    }

    // ---- Start Menu shortcut -------------------------------------------

    fn start_menu_path() -> Result<PathBuf> {
        let appdata = std::env::var_os("APPDATA").context("APPDATA is not set")?;
        Ok(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\pm.lnk"))
    }

    fn create_start_menu_shortcut(exe: &Path) -> Result<PathBuf> {
        let lnk = start_menu_path()?;
        let dir = exe.parent().unwrap_or_else(|| Path::new("."));
        let lnk_s = psq(&lnk.to_string_lossy());
        let exe_s = psq(&exe.to_string_lossy());
        let dir_s = psq(&dir.to_string_lossy());
        let script = format!(
            "$w=New-Object -ComObject WScript.Shell; \
             $s=$w.CreateShortcut('{lnk_s}'); \
             $s.TargetPath='{exe_s}'; \
             $s.WorkingDirectory='{dir_s}'; \
             $s.IconLocation='{exe_s},0'; \
             $s.Description='pm — Plus Minus'; \
             $s.Save()"
        );
        ps(&script)?;
        Ok(lnk)
    }

    fn remove_start_menu_shortcut() -> Result<()> {
        let lnk = start_menu_path()?;
        match std::fs::remove_file(&lnk) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).context("deleting the Start Menu shortcut"),
        }
    }

    // ---- self-delete ---------------------------------------------------

    fn schedule_self_delete(exe: &Path) -> Result<()> {
        // A running exe can't be deleted, but it can be renamed; hand the
        // leftover to a detached `cmd` that waits for us to exit first.
        let old = exe.with_extension("exe.old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old).context("moving pm.exe aside")?;
        Command::new("cmd")
            .arg("/c")
            .arg("ping 127.0.0.1 -n 3 >nul & del /f /q")
            .arg(&old)
            .spawn()
            .context("spawning the cleanup command")?;
        Ok(())
    }

    // ---- MCP client wiring (Claude Code) -----------------------------

    fn claude_cli() -> Option<&'static str> {
        for c in ["claude", "claude.cmd"] {
            let ok = Command::new(c)
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if ok {
                return Some(c);
            }
        }
        None
    }

    pub(super) fn mcp_plug() -> Result<String> {
        if let Some(cli) = claude_cli() {
            let out = Command::new(cli)
                .args(["mcp", "add", "pm", "-s", "user", "--", "pm", "--mcp"])
                .output()
                .context("running `claude mcp add`")?;
            let err = String::from_utf8_lossy(&out.stderr);
            if out.status.success() || err.contains("already exists") {
                return Ok("registered via the claude CLI (user scope)".into());
            }
            eprintln!(
                "  `claude mcp add` failed ({}); falling back to ~/.claude.json",
                err.trim()
            );
        }
        claude_json_edit(true)?;
        Ok("wrote ~/.claude.json".into())
    }

    pub(super) fn mcp_unplug() -> Result<()> {
        if let Some(cli) = claude_cli() {
            let _ = Command::new(cli)
                .args(["mcp", "remove", "pm", "-s", "user"])
                .output();
        }
        claude_json_edit(false)
    }

    fn claude_json_path() -> Result<PathBuf> {
        #[allow(deprecated)]
        let home = std::env::home_dir().context("no home directory")?;
        Ok(home.join(".claude.json"))
    }

    /// Add or remove the `mcpServers.pm` entry in `~/.claude.json`, leaving every
    /// other key untouched. No-op when already in the desired state.
    fn claude_json_edit(add: bool) -> Result<()> {
        let path = claude_json_path()?;
        let mut root: serde_json::Value = match std::fs::read_to_string(&path) {
            Ok(s) if s.trim().is_empty() => serde_json::json!({}),
            Ok(s) => serde_json::from_str(&s).context("parsing ~/.claude.json")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
            Err(e) => return Err(e).context("reading ~/.claude.json"),
        };
        let obj = root
            .as_object_mut()
            .context("~/.claude.json is not a JSON object")?;
        let servers = obj
            .entry("mcpServers")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("mcpServers in ~/.claude.json is not an object")?;

        let changed = if add {
            let want =
                serde_json::json!({ "type": "stdio", "command": "pm", "args": ["--mcp"] });
            if servers.get("pm") == Some(&want) {
                false
            } else {
                servers.insert("pm".into(), want);
                true
            }
        } else {
            servers.remove("pm").is_some()
        };

        if changed {
            let mut s = serde_json::to_string_pretty(&root)?;
            s.push('\n');
            std::fs::write(&path, s).context("writing ~/.claude.json")?;
        }
        Ok(())
    }
}
