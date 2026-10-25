//! Finds the Dota 2 install, so `dota2coach setup` can write the GSI config
//! without the player hunting for folders.
//!
//! Steam keeps games in one or more library folders, listed in
//! `<steam>/steamapps/libraryfolders.vdf`. Dota may be in any of them, for
//! example `F:\SteamLibrary` rather than under Program Files.

use std::path::{Path, PathBuf};

/// Dota's folder name under `steamapps/common`.
const DOTA_DIR: &str = "dota 2 beta";

/// Where GSI configs go, relative to the Dota install.
pub const GSI_SUBDIR: [&str; 4] = ["game", "dota", "cfg", "gamestate_integration"];

/// The Dota 2 install folder, from the first Steam library that has it.
pub fn find_dota() -> Option<PathBuf> {
    steam_roots()
        .iter()
        .flat_map(|root| libraries(root))
        .map(|lib| lib.join("steamapps").join("common").join(DOTA_DIR))
        .find(|dir| dir.join("game").is_dir())
}

/// The `gamestate_integration` folder inside a Dota install.
pub fn gsi_dir(dota: &Path) -> PathBuf {
    GSI_SUBDIR
        .iter()
        .fold(dota.to_owned(), |p, part| p.join(part))
}

/// Steam install folders to look in, most likely first.
fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(windows)]
    {
        if let Some(path) = registry_steam_path() {
            roots.push(path);
        }
        for var in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(dir) = std::env::var_os(var) {
                roots.push(PathBuf::from(dir).join("Steam"));
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join("Library/Application Support/Steam"));
        roots.push(home.join(".local/share/Steam"));
        roots.push(home.join(".steam/steam"));
        roots.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
    }
    roots.retain(|r| r.is_dir());
    roots
}

/// `HKCU\Software\Valve\Steam\SteamPath`, read with the built-in `reg` tool.
#[cfg(windows)]
fn registry_steam_path() -> Option<PathBuf> {
    let out = std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains("SteamPath"))?;
    let value = line.split("REG_SZ").nth(1)?.trim();
    Some(PathBuf::from(value.replace('/', "\\")))
}

/// The Steam root itself plus every library listed in `libraryfolders.vdf`.
fn libraries(root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![root.to_owned()];
    if let Ok(text) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        libs.extend(library_paths(&text).into_iter().map(PathBuf::from));
    }
    libs
}

/// The `"path"` values in a `libraryfolders.vdf` file, unescaped.
fn library_paths(vdf: &str) -> Vec<String> {
    vdf.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("\"path\"")?.trim();
            let value = rest.strip_prefix('"')?.strip_suffix('"')?;
            Some(value.replace("\\\\", "\\"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_library_paths() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"apps"
		{
			"228980"		"423542356"
		}
	}
	"1"
	{
		"path"		"F:\\SteamLibrary"
		"apps"
		{
			"570"		"61234567890"
		}
	}
}
"#;
        assert_eq!(
            library_paths(vdf),
            vec![r"C:\Program Files (x86)\Steam", r"F:\SteamLibrary"]
        );
    }

    #[test]
    fn finds_dota_in_a_second_library() {
        let root = std::env::temp_dir().join(format!("dota2coach-steam-{}", std::process::id()));
        let lib = root.join("lib2");
        let dota = lib.join("steamapps/common").join(DOTA_DIR);
        std::fs::create_dir_all(dota.join("game")).unwrap();
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        let vdf = format!(
            "\"libraryfolders\"\n{{\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
            lib.display().to_string().replace('\\', "\\\\")
        );
        std::fs::write(root.join("steamapps/libraryfolders.vdf"), vdf).unwrap();

        let found = libraries(&root)
            .into_iter()
            .map(|l| l.join("steamapps/common").join(DOTA_DIR))
            .find(|d| d.join("game").is_dir());
        assert_eq!(found, Some(dota.clone()));
        assert!(gsi_dir(&dota).ends_with("game/dota/cfg/gamestate_integration"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
