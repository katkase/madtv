//! dos.library emulata: file, lock, directory, console.
//!
//! Le strutture restituite al programma (FileHandle, FileLock) sono vere
//! strutture allocate nella memoria emulata, ma il loro contenuto è opaco: il
//! programma le usa solo come "maniglie" (puntatori BCPL, BPTR = indirizzo/4).

use super::vfs::Resolved;
use super::{Ctx, LockInfo, OpenFile, Os};

const MODE_READWRITE: u32 = 1004;
const MODE_OLDFILE: u32 = 1005;
const MODE_NEWFILE: u32 = 1006;

const ERROR_OBJECT_NOT_FOUND: u32 = 205;
const ERROR_OBJECT_WRONG_TYPE: u32 = 212;
const ERROR_NO_MORE_ENTRIES: u32 = 232;
const ERROR_WRITE_PROTECTED: u32 = 214;

/// Maniglia speciale per la console (Input/Output e "*").
pub const CONSOLE_PATH: &str = "*CONSOLE*";

pub fn call(os: &mut Os, c: &mut Ctx, idx: u16) -> Option<String> {
    Some(match idx {
        // Open(name d1, accessMode d2) -> BPTR FileHandle
        0 => {
            let name = c.bus.cstr(c.d(1));
            let mode = c.d(2);
            let fh = open(os, c, &name, mode);
            c.set_d0(fh);
            format!("(\"{name}\", {mode})")
        }
        // Close(file d1)
        1 => {
            let fh = c.d(1);
            let desc = close(os, c, fh);
            c.set_d0(1);
            desc
        }
        // Read(file d1, buffer d2, length d3) -> byte letti
        2 => {
            let (fh, buf, len) = (c.d(1), c.d(2), c.d(3) as i32);
            let n = match os.files.get_mut(&fh) {
                Some(f) => {
                    let avail = f.data.len().saturating_sub(f.pos);
                    let n = (len.max(0) as usize).min(avail);
                    let slice = f.data[f.pos..f.pos + n].to_vec();
                    f.pos += n;
                    c.bus.write_bytes(buf, &slice);
                    n as u32
                }
                None => 0xFFFF_FFFF,
            };
            c.set_d0(n);
            format!("(fh={fh:x}, buf={buf:06x}, len={len})")
        }
        // Write(file d1, buffer d2, length d3)
        3 => {
            let (fh, buf, len) = (c.d(1), c.d(2), c.d(3) as i32);
            let data = c.bus.read_bytes(buf, len.max(0) as usize);
            let n = match os.files.get_mut(&fh) {
                Some(f) if f.path.path == CONSOLE_PATH => {
                    os.console.extend_from_slice(&data);
                    len as u32
                }
                Some(f) if f.writable => {
                    let end = f.pos + data.len();
                    if f.data.len() < end {
                        f.data.resize(end, 0);
                    }
                    f.data[f.pos..end].copy_from_slice(&data);
                    f.pos = end;
                    f.dirty = true;
                    len as u32
                }
                _ => {
                    os.io_err = ERROR_WRITE_PROTECTED;
                    0xFFFF_FFFF
                }
            };
            c.set_d0(n);
            format!(
                "(fh={fh:x}, len={len}) {:?}",
                String::from_utf8_lossy(&data[..data.len().min(60)])
            )
        }
        // Input() / Output()
        4 => {
            c.set_d0(os.stdin_fh);
            String::new()
        }
        5 => {
            c.set_d0(os.stdout_fh);
            String::new()
        }
        // Seek(file d1, position d2, mode d3) -> posizione precedente
        6 => {
            let (fh, pos, mode) = (c.d(1), c.d(2) as i32, c.d(3) as i32);
            let r = match os.files.get_mut(&fh) {
                Some(f) => {
                    let old = f.pos as i64;
                    let base = match mode {
                        -1 => 0i64,
                        0 => old,
                        _ => f.data.len() as i64,
                    };
                    let new = base + pos as i64;
                    if new < 0 || new > f.data.len() as i64 {
                        0xFFFF_FFFF
                    } else {
                        f.pos = new as usize;
                        old as u32
                    }
                }
                None => 0xFFFF_FFFF,
            };
            c.set_d0(r);
            format!("(fh={fh:x}, {pos}, mode={mode})")
        }
        // DeleteFile(name d1)
        7 => {
            let name = c.bus.cstr(c.d(1));
            let ok = os
                .vfs
                .resolve(&name, &os.cur_dir)
                .map(|r| os.vfs.delete(&r))
                .unwrap_or(false);
            c.set_d0(if ok { 0xFFFF_FFFF } else { 0 });
            format!("(\"{name}\")")
        }
        // Lock(name d1, type d2) -> BPTR FileLock
        9 => {
            let name = c.bus.cstr(c.d(1));
            let r = os
                .vfs
                .resolve(&name, &os.cur_dir)
                .filter(|r| os.vfs.exists(r));
            let l = match r {
                Some(r) => new_lock(os, c, r),
                None => {
                    os.io_err = ERROR_OBJECT_NOT_FOUND;
                    0
                }
            };
            c.set_d0(l);
            format!("(\"{name}\", {})", c.d(2) as i32)
        }
        // UnLock(lock d1)
        10 => {
            let l = c.d(1);
            if os.locks.remove(&l).is_some() {
                os.heap.free(l << 2);
            }
            String::new()
        }
        // DupLock(lock d1)
        11 => {
            let l = c.d(1);
            let path = lock_path(os, l);
            let n = new_lock(os, c, path);
            c.set_d0(n);
            String::new()
        }
        // Examine(lock d1, fib d2)
        12 => {
            let (l, fib) = (c.d(1), c.d(2));
            let path = lock_path(os, l);
            let is_dir = os.vfs.is_dir(&path);
            let name = if path.path.is_empty() {
                os.vfs.volumes[path.vol].name.clone()
            } else {
                path.path.rsplit('/').next().unwrap_or("").to_string()
            };
            let size = if is_dir {
                0
            } else {
                os.vfs.read(&path).map(|d| d.len() as u32).unwrap_or(0)
            };
            fill_fib(c, fib, &name, is_dir, size);
            if let Some(li) = os.locks.get_mut(&l) {
                li.exnext = 0;
            }
            c.set_d0(0xFFFF_FFFF);
            format!("(\"{}\")", os.vfs.display(&path))
        }
        // ExNext(lock d1, fib d2)
        13 => {
            let (l, fib) = (c.d(1), c.d(2));
            let path = lock_path(os, l);
            let list = os.vfs.list(&path);
            let i = os.locks.get(&l).map(|x| x.exnext).unwrap_or(0);
            if let Some((name, is_dir, size)) = list.get(i) {
                fill_fib(c, fib, name, *is_dir, *size);
                if let Some(li) = os.locks.get_mut(&l) {
                    li.exnext += 1;
                }
                c.set_d0(0xFFFF_FFFF);
                format!("-> \"{name}\"")
            } else {
                os.io_err = ERROR_NO_MORE_ENTRIES;
                c.set_d0(0);
                "-> fine".to_string()
            }
        }
        // Info(lock d1, parameterBlock d2): disco protetto da scrittura, 880 KB.
        14 => {
            let p = c.d(2);
            for i in 0..36 {
                c.bus.poke8(p + i, 0);
            }
            c.bus.poke32(p + 8, 80); // id_DiskState = ID_WRITE_PROTECTED
            c.bus.poke32(p + 12, 1760); // id_NumBlocks
            c.bus.poke32(p + 16, 1760); // id_NumBlocksUsed
            c.bus.poke32(p + 20, 512); // id_BytesPerBlock
            c.bus.poke32(p + 24, 0x444F_5300); // id_DiskType = 'DOS\0'
            c.set_d0(0xFFFF_FFFF);
            String::new()
        }
        // CurrentDir(lock d1) -> lock precedente
        16 => {
            let l = c.d(1);
            let old = c.bus.peek32(os.process + 152);
            c.bus.poke32(os.process + 152, l);
            os.cur_dir = lock_path(os, l);
            c.set_d0(old);
            format!("(\"{}\")", os.vfs.display(&os.cur_dir))
        }
        // IoErr()
        17 => {
            c.set_d0(os.io_err);
            String::new()
        }
        // Exit(returnCode d1)
        19 => format!("EXIT: {}", c.d(1) as i32),
        // DateStamp(ds d1): giorni dal 1/1/1978, minuti, tick (1/50 s)
        27 => {
            let ds = c.d(1);
            let now = match os.clock_override {
                // Orologio fisso + tempo emulato (50 frame al secondo).
                Some(base) => base + c.bus.custom.frame / 50,
                None => std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            };
            let amiga = now.saturating_sub(252_460_800); // 1978-01-01
            c.bus.poke32(ds, (amiga / 86400) as u32);
            c.bus.poke32(ds + 4, ((amiga % 86400) / 60) as u32);
            c.bus.poke32(ds + 8, ((amiga % 60) * 50) as u32);
            c.set_d0(ds);
            String::new()
        }
        // Delay(timeout d1): avanza il tempo emulato di `timeout` cinquantesimi di secondo.
        28 => {
            let t = c.d(1) as u64;
            // Il tempo avanza nel ciclo principale, che consegna anche i vertical blank.
            os.pending_delay += t;
            format!("({t})")
        }
        // IsInteractive(file d1)
        31 => {
            let fh = c.d(1);
            let inter = os
                .files
                .get(&fh)
                .map(|f| f.path.path == CONSOLE_PATH)
                .unwrap_or(false);
            c.set_d0(if inter { 0xFFFF_FFFF } else { 0 });
            String::new()
        }
        _ => return None,
    })
}

fn open(os: &mut Os, c: &mut Ctx, name: &str, mode: u32) -> u32 {
    let upper = name.to_uppercase();
    if name == "*" || upper.starts_with("CON:") || upper.starts_with("RAW:") || upper == "NIL:" {
        return new_handle(os, c, console_resolved(), Vec::new(), true);
    }
    let Some(r) = os.vfs.resolve(name, &os.cur_dir) else {
        os.io_err = ERROR_OBJECT_NOT_FOUND;
        return 0;
    };
    match mode {
        MODE_NEWFILE => new_handle(os, c, r, Vec::new(), true),
        MODE_OLDFILE | MODE_READWRITE => match os.vfs.read(&r) {
            Some(data) => new_handle(os, c, r, data, mode == MODE_READWRITE),
            None if mode == MODE_READWRITE => new_handle(os, c, r, Vec::new(), true),
            None => {
                os.io_err = if os.vfs.is_dir(&r) {
                    ERROR_OBJECT_WRONG_TYPE
                } else {
                    ERROR_OBJECT_NOT_FOUND
                };
                0
            }
        },
        _ => {
            os.io_err = ERROR_OBJECT_NOT_FOUND;
            0
        }
    }
}

pub fn console_resolved() -> Resolved {
    Resolved {
        vol: 0,
        path: CONSOLE_PATH.to_string(),
    }
}

pub fn new_handle(os: &mut Os, c: &mut Ctx, path: Resolved, data: Vec<u8>, writable: bool) -> u32 {
    let a = os.alloc_clear(c.bus, 44); // struct FileHandle
    let bptr = a >> 2;
    let dirty = writable && path.path != CONSOLE_PATH && data.is_empty();
    os.files.insert(
        bptr,
        OpenFile {
            path,
            data,
            pos: 0,
            writable,
            dirty,
        },
    );
    bptr
}

fn close(os: &mut Os, c: &mut Ctx, fh: u32) -> String {
    let Some(f) = os.files.remove(&fh) else {
        return format!("(fh={fh:x}) [sconosciuto]");
    };
    os.heap.free(fh << 2);
    let _ = c;
    if f.dirty && f.path.path != CONSOLE_PATH {
        match os.vfs.write(&f.path, &f.data) {
            Ok(()) => format!(
                "(\"{}\") salvato, {} byte",
                os.vfs.display(&f.path),
                f.data.len()
            ),
            Err(e) => format!("(\"{}\") ERRORE di scrittura: {e}", os.vfs.display(&f.path)),
        }
    } else {
        format!("(\"{}\")", os.vfs.display(&f.path))
    }
}

pub fn new_lock(os: &mut Os, c: &mut Ctx, path: Resolved) -> u32 {
    let a = os.alloc_clear(c.bus, 20); // struct FileLock
    let bptr = a >> 2;
    os.locks.insert(bptr, LockInfo { path, exnext: 0 });
    bptr
}

/// Percorso associato a un lock; il lock 0 indica la radice del disco di avvio.
fn lock_path(os: &Os, l: u32) -> Resolved {
    if l == 0 {
        return Resolved {
            vol: 0,
            path: String::new(),
        };
    }
    os.locks
        .get(&l)
        .map(|x| x.path.clone())
        .unwrap_or(Resolved {
            vol: 0,
            path: String::new(),
        })
}

/// Compila una struct FileInfoBlock.
fn fill_fib(c: &mut Ctx, fib: u32, name: &str, is_dir: bool, size: u32) {
    for i in 0..260 {
        c.bus.poke8(fib + i, 0);
    }
    let t: i32 = if is_dir { 2 } else { -3 };
    c.bus.poke32(fib + 4, t as u32); // fib_DirEntryType
    let n = name.len().min(106);
    c.bus.poke8(fib + 8, n as u8); // fib_FileName (BSTR)
    c.bus.write_bytes(fib + 9, &name.as_bytes()[..n]);
    c.bus.poke32(fib + 120, t as u32); // fib_EntryType
    c.bus.poke32(fib + 124, size); // fib_Size
    c.bus.poke32(fib + 128, size.div_ceil(488)); // fib_NumBlocks
}
