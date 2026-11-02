//! File system virtuale per il dos.library emulato.
//!
//! - Volumi in sola lettura dalle immagini ADF, accessibili per nome di volume
//!   (`MTV1:`, `MTV2:`) e con gli alias `DF0:`/`SYS:` (disco 1) e `DF1:` (disco 2).
//! - Le scritture (salvataggi) finiscono in una cartella del PC
//!   (`<save_dir>/<VOLUME>/<percorso>`), che viene consultata per prima anche in
//!   lettura: così i salvataggi sopravvivono tra una sessione e l'altra senza
//!   toccare gli ADF originali.

use crate::adf::Adf;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Volume {
    pub name: String,
    /// Condiviso: le copie della macchina (vedi `Machine::clone`) non duplicano i dischi.
    pub adf: std::sync::Arc<Adf>,
}

#[derive(Clone)]
pub struct Vfs {
    pub volumes: Vec<Volume>,
    pub save_dir: PathBuf,
    /// Se vero le scritture vengono scartate: serve alle copie "di prova"
    /// della macchina, che non devono toccare i salvataggi veri.
    pub read_only: bool,
}

/// Percorso risolto: volume (indice) + percorso interno in stile AmigaDOS.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub vol: usize,
    pub path: String,
}

impl Vfs {
    pub fn new(adfs: Vec<Adf>, save_dir: PathBuf) -> Vfs {
        let volumes = adfs
            .into_iter()
            .map(|adf| Volume {
                name: adf.volume_name.clone(),
                adf: std::sync::Arc::new(adf),
            })
            .collect();
        Vfs {
            volumes,
            save_dir,
            read_only: false,
        }
    }

    /// Risolve un nome AmigaDOS rispetto alla directory corrente.
    pub fn resolve(&self, name: &str, cur: &Resolved) -> Option<Resolved> {
        let (vol, rest) = match name.find(':') {
            Some(i) => {
                let v = name[..i].to_uppercase();
                let vol = match v.as_str() {
                    "" => cur.vol,
                    "DF0" | "SYS" | "S" | "C" | "L" | "DEVS" | "LIBS" | "FONTS" => 0,
                    "DF1" => 1.min(self.volumes.len() - 1),
                    _ => self
                        .volumes
                        .iter()
                        .position(|x| x.name.eq_ignore_ascii_case(&v))?,
                };
                // Gli assegnamenti logici puntano a sottodirectory del disco di sistema.
                let prefix = match v.as_str() {
                    "S" => "s/",
                    "C" => "c/",
                    "L" => "l/",
                    "DEVS" => "devs/",
                    "LIBS" => "libs/",
                    "FONTS" => "fonts/",
                    _ => "",
                };
                (vol, format!("{prefix}{}", &name[i + 1..]))
            }
            None => {
                let base = cur.path.trim_end_matches('/');
                let p = if base.is_empty() {
                    name.to_string()
                } else {
                    format!("{base}/{name}")
                };
                (cur.vol, p)
            }
        };
        Some(Resolved {
            vol,
            path: normalize(&rest),
        })
    }

    fn host_path(&self, r: &Resolved) -> PathBuf {
        let mut p = self.save_dir.join(&self.volumes[r.vol].name);
        for part in r.path.split('/').filter(|s| !s.is_empty()) {
            p.push(part);
        }
        p
    }

    pub fn exists(&self, r: &Resolved) -> bool {
        r.path.is_empty()
            || self.host_path(r).exists()
            || self.volumes[r.vol].adf.find(&r.path).is_some()
    }

    pub fn is_dir(&self, r: &Resolved) -> bool {
        if r.path.is_empty() {
            return true;
        }
        let hp = self.host_path(r);
        if hp.exists() {
            return hp.is_dir();
        }
        self.volumes[r.vol]
            .adf
            .find(&r.path)
            .map(|e| e.is_dir)
            .unwrap_or(false)
    }

    pub fn read(&self, r: &Resolved) -> Option<Vec<u8>> {
        let hp = self.host_path(r);
        if hp.is_file() {
            return std::fs::read(hp).ok();
        }
        self.volumes[r.vol].adf.read_file(&r.path).ok()
    }

    pub fn write(&self, r: &Resolved, data: &[u8]) -> std::io::Result<()> {
        if self.read_only {
            return Ok(());
        }
        let hp = self.host_path(r);
        if let Some(parent) = hp.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(hp, data)
    }

    pub fn delete(&self, r: &Resolved) -> bool {
        if self.read_only {
            return false;
        }
        let hp = self.host_path(r);
        hp.is_file() && std::fs::remove_file(hp).is_ok()
    }

    /// Elenco di una directory: (nome, è_directory, dimensione).
    pub fn list(&self, r: &Resolved) -> Vec<(String, bool, u32)> {
        let mut out: Vec<(String, bool, u32)> = Vec::new();
        let prefix = if r.path.is_empty() {
            String::new()
        } else {
            format!("{}/", r.path.to_uppercase())
        };
        for e in self.volumes[r.vol].adf.entries() {
            let up = e.name.to_uppercase();
            if let Some(rest) = up.strip_prefix(&prefix)
                && !rest.contains('/')
            {
                let name = e.name.rsplit('/').next().unwrap_or(&e.name).to_string();
                out.push((name, e.is_dir, e.size));
            }
        }
        let hp = self.host_path(r);
        if let Ok(rd) = std::fs::read_dir(&hp) {
            for de in rd.flatten() {
                let name = de.file_name().to_string_lossy().to_string();
                let md = de.metadata().ok();
                let is_dir = md.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                let size = md.map(|m| m.len() as u32).unwrap_or(0);
                if let Some(x) = out.iter_mut().find(|x| x.0.eq_ignore_ascii_case(&name)) {
                    *x = (name, is_dir, size);
                } else {
                    out.push((name, is_dir, size));
                }
            }
        }
        out
    }

    pub fn display(&self, r: &Resolved) -> String {
        format!("{}:{}", self.volumes[r.vol].name, r.path)
    }

    pub fn save_root(&self) -> &Path {
        &self.save_dir
    }
}

/// Normalizza un percorso AmigaDOS: separatori multipli e "/" iniziale
/// (che in AmigaDOS significa "directory padre").
fn normalize(p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut rest = p;
    // In AmigaDOS ogni "/" iniziale risale di un livello.
    while let Some(r) = rest.strip_prefix('/') {
        parts.pop();
        rest = r;
    }
    // Una "/" finale non cambia il percorso (es. "c/" = "c").
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    for seg in rest.split('/') {
        if seg.is_empty() {
            parts.pop();
        } else {
            parts.push(seg);
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn percorsi_amigados() {
        assert_eq!(normalize("a/b"), "a/b");
        assert_eq!(normalize("a//b"), "b"); // "//" risale di un livello
        assert_eq!(normalize("/x"), "x");
        assert_eq!(normalize("a/b/"), "a/b");
    }
}
