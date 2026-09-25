//! Load undirected ASCII edge lists into a C-compatible CSR graph.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::{CStr, CString, c_char};
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::panic::catch_unwind;
use std::ptr;

#[repr(C)]
pub struct Csr {
    pub vertex_count: u64,
    pub edge_count: u64,
    pub row_offsets: *const u64,
    pub column_indices: *const u64,
}

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

#[derive(Debug, PartialEq)]
struct GraphData {
    row_offsets: Vec<u64>,
    column_indices: Vec<u64>,
}

// The public Csr is the first field, so its pointer also identifies its owner.
// Retaining the Vecs preserves their allocation capacities without a shrink/copy.
#[repr(C)]
struct OwnedCsr {
    csr: Csr,
    _data: GraphData,
}

impl GraphData {
    fn into_raw(self) -> *mut Csr {
        let csr = Csr {
            vertex_count: (self.row_offsets.len() - 1) as u64,
            edge_count: (self.column_indices.len() / 2) as u64,
            row_offsets: self.row_offsets.as_ptr(),
            column_indices: self.column_indices.as_ptr(),
        };
        Box::into_raw(Box::new(OwnedCsr { csr, _data: self })).cast::<Csr>()
    }
}

fn parse_id(bytes: &[u8]) -> Option<u64> {
    let mut value = 0u64;
    for &byte in bytes {
        let digit = byte.wrapping_sub(b'0');
        if digit > 9 {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(u64::from(digit))?;
    }
    Some(value)
}

fn parse_edge(line: &[u8]) -> Option<(u64, u64)> {
    let mut fields = line
        .split(u8::is_ascii_whitespace)
        .filter(|field| !field.is_empty());
    let u = fields.next()?;
    let v = fields.next()?;
    if fields.next().is_some() {
        return None;
    }
    Some((parse_id(u)?, parse_id(v)?))
}

fn read_graph(mut reader: impl BufRead) -> io::Result<GraphData> {
    let mut adj: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    let mut line = Vec::with_capacity(64);
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        let Some((u, v)) = parse_edge(&line) else {
            continue;
        };
        if u == v {
            continue;
        }
        adj.entry(u).or_default().insert(v);
        adj.entry(v).or_default().insert(u);
    }

    let mut new_id = HashMap::with_capacity(adj.len());
    for (index, &id) in adj.keys().enumerate() {
        new_id.insert(id, index as u64);
    }
    let entries: usize = adj.values().map(BTreeSet::len).sum();
    let mut row_offsets = Vec::with_capacity(adj.len() + 1);
    let mut column_indices = Vec::with_capacity(entries);
    row_offsets.push(0);
    for neighbors in adj.values() {
        column_indices.extend(neighbors.iter().map(|id| new_id[id]));
        row_offsets.push(column_indices.len() as u64);
    }
    Ok(GraphData {
        row_offsets,
        column_indices,
    })
}

fn set_error(message: String) {
    // C strings cannot contain interior NUL bytes, including in an OS error.
    let message = CString::new(message.replace('\0', "\\0")).expect("NUL bytes were replaced");
    LAST_ERROR.with(|error| *error.borrow_mut() = Some(message));
}

/// Load an edge-list file. Returns null on failure; see rere_last_error.
///
/// # Safety
/// A non-null path must point to a readable, NUL-terminated byte string for the
/// duration of this call. Release a successful result with rere_free_csr.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rere_load_csr(path: *const c_char) -> *mut Csr {
    LAST_ERROR.with(|error| *error.borrow_mut() = None);
    let result = catch_unwind(|| -> io::Result<*mut Csr> {
        if path.is_null() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "path is null"));
        }
        // SAFETY: the caller supplies a readable, NUL-terminated string.
        let path = unsafe { CStr::from_ptr(path) };
        let path = path
            .to_str()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let file = File::open(path)?;
        read_graph(BufReader::with_capacity(1024 * 1024, file)).map(GraphData::into_raw)
    });
    match result {
        Ok(Ok(csr)) => csr,
        Ok(Err(error)) => {
            set_error(error.to_string());
            ptr::null_mut()
        }
        Err(_) => {
            set_error("panic while loading graph".to_owned());
            ptr::null_mut()
        }
    }
}

/// Return this thread's last load error, or null if there is no error.
///
/// The library owns the string. It is valid until the next load call on the
/// same thread, or until the thread exits. Do not free it.
#[unsafe(no_mangle)]
pub extern "C" fn rere_last_error() -> *const c_char {
    LAST_ERROR.with(|error| error.borrow().as_ref().map_or(ptr::null(), |s| s.as_ptr()))
}

/// Release a graph and both CSR arrays. A null pointer is allowed.
///
/// # Safety
/// A non-null csr must be the original, unfreed pointer returned by rere_load_csr.
/// No graph or array references may be used after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rere_free_csr(csr: *mut Csr) {
    if !csr.is_null() {
        // SAFETY: OwnedCsr is repr(C) with Csr first, and the caller passes the
        // original live pointer returned by GraphData::into_raw.
        unsafe { drop(Box::from_raw(csr.cast::<OwnedCsr>())) };
    }
}

#[cfg(test)]
mod tests;
