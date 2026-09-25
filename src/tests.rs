use super::*;
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(contents: &[u8]) -> Self {
        static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
        for _ in 0..32 {
            let path = std::env::temp_dir().join(format!(
                "librere-test-{}-{}.edges",
                std::process::id(),
                NEXT_FILE.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let result = file.write_all(contents);
                    drop(file);
                    let fixture = Self { path };
                    result.expect("write temporary edge list");
                    return fixture;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create temporary edge list: {error}"),
            }
        }
        panic!("could not find an unused temporary filename");
    }

    fn c_path(&self) -> CString {
        CString::new(self.path.to_str().expect("UTF-8 temporary path")).unwrap()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct LoadedGraph(*mut Csr);

impl LoadedGraph {
    fn snapshot(&self) -> GraphData {
        assert!(!self.0.is_null(), "load failed: {:?}", last_error_text());
        // SAFETY: this guard owns a successful load result until its Drop runs.
        unsafe {
            let csr = &*self.0;
            let row_offsets =
                std::slice::from_raw_parts(csr.row_offsets, csr.vertex_count as usize + 1).to_vec();
            let entries = *row_offsets.last().unwrap();
            assert_eq!(entries, 2 * csr.edge_count);
            let column_indices =
                std::slice::from_raw_parts(csr.column_indices, entries as usize).to_vec();
            GraphData {
                row_offsets,
                column_indices,
            }
        }
    }
}

impl Drop for LoadedGraph {
    fn drop(&mut self) {
        // SAFETY: the guard owns this result and releases it exactly once.
        unsafe { rere_free_csr(self.0) };
    }
}

fn last_error_text() -> Option<String> {
    let error = rere_last_error();
    if error.is_null() {
        None
    } else {
        // SAFETY: copy the library-owned string before any subsequent load.
        Some(
            unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}

fn graph(rows: &[u64], columns: &[u64]) -> GraphData {
    GraphData {
        row_offsets: rows.to_vec(),
        column_indices: columns.to_vec(),
    }
}

#[test]
fn csr_orders_sparse_ids_and_neighbors_and_deduplicates_both_directions() {
    let input = b"90 10\n30 90\n10 30\n90 10\n10 90\n700 30\n";
    assert_eq!(
        read_graph(Cursor::new(input)).unwrap(),
        graph(&[0, 2, 5, 7, 8], &[1, 2, 0, 2, 3, 0, 1, 1])
    );
}

#[test]
fn empty_and_self_loop_only_inputs_have_no_vertices() {
    for input in [b"".as_slice(), b" \t\r\n# comment\n", b"0 0\n9 9\n"] {
        assert_eq!(read_graph(Cursor::new(input)).unwrap(), graph(&[0], &[]));
    }
    assert_eq!(
        read_graph(Cursor::new(b"1 1\n5 8\n99 99\n")).unwrap(),
        graph(&[0, 1, 2], &[1, 0])
    );
}

#[test]
fn maximum_id_is_valid_but_overflow_and_extra_fields_cannot_be_salvaged() {
    assert_eq!(parse_edge(b"18446744073709551615 0"), Some((u64::MAX, 0)));
    assert_eq!(parse_edge(b"0000000000000000000000000000 01"), Some((0, 1)));
    for input in [
        b"1 18446744073709551616".as_slice(),
        b"1 18446744073709551616 2",
        b"18446744073709551616 1 2",
        b"1 2 18446744073709551616",
        b"1 2 3",
        b"-1 2",
        b"+1 2",
        b"0x1 2",
        b"1",
        b"",
    ] {
        assert_eq!(parse_edge(input), None, "input: {input:?}");
    }
    let input = concat!(
        "0 18446744073709551615\n",
        "1 18446744073709551616 2\n",
        "2 18446744073709551616\n",
        "7 8 9\n"
    );
    assert_eq!(
        read_graph(Cursor::new(input.as_bytes())).unwrap(),
        graph(&[0, 1, 2], &[1, 0])
    );
}

#[test]
fn crlf_tabs_and_final_line_work_while_non_ascii_lines_are_skipped_whole() {
    let input = b"  40\t10\r\n1\xC2\xA02\r\n3 4\xFF\r\n\xEF\xBC\x91 5\r\n10 20\r\n20\t40";
    assert_eq!(
        read_graph(Cursor::new(input)).unwrap(),
        graph(&[0, 2, 4, 6], &[1, 2, 0, 2, 0, 1])
    );
}

#[test]
fn read_error_after_valid_edges_does_not_return_a_partial_graph() {
    struct FailingRead {
        remaining: &'static [u8],
    }
    impl Read for FailingRead {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if output.is_empty() {
                return Ok(0);
            }
            if self.remaining.is_empty() {
                return Err(io::Error::other("injected read failure"));
            }
            let count = output.len().min(self.remaining.len());
            output[..count].copy_from_slice(&self.remaining[..count]);
            self.remaining = &self.remaining[count..];
            Ok(count)
        }
    }
    let reader = BufReader::with_capacity(
        4,
        FailingRead {
            remaining: b"1 2\n3 ",
        },
    );
    let error = read_graph(reader).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(error.to_string(), "injected read failure");
}

#[test]
fn ffi_reports_null_invalid_utf8_and_missing_paths() {
    let invalid_utf8 = [0xFFu8, 0];
    let fixture = TempFile::new(b"");
    let missing_path = fixture.c_path();
    fs::remove_file(&fixture.path).unwrap();
    for path in [
        ptr::null(),
        invalid_utf8.as_ptr().cast(),
        missing_path.as_ptr(),
    ] {
        // SAFETY: each non-null path is a readable NUL-terminated byte string;
        // invalid UTF-8 is deliberately submitted to the validation boundary.
        let result = unsafe { rere_load_csr(path) };
        let owner = LoadedGraph(result);
        assert!(owner.0.is_null());
        assert!(last_error_text().is_some_and(|message| !message.is_empty()));
    }
}

#[test]
fn ffi_success_clears_errors_and_both_entry_points_can_be_loaded_and_freed() {
    let fixture = TempFile::new(b"90 10\n30 90\n10 30\n10 90\n");
    let path = fixture.c_path();
    for _ in 0..16 {
        // SAFETY: null is explicitly checked by the loader.
        assert!(unsafe { rere_load_csr(ptr::null()) }.is_null());
        assert!(last_error_text().is_some());
        // SAFETY: path remains alive throughout the call and is valid UTF-8.
        let owner = LoadedGraph(unsafe { rere_load_csr(path.as_ptr()) });
        assert_eq!(owner.snapshot(), graph(&[0, 2, 4, 6], &[1, 2, 0, 2, 0, 1]));
        assert!(rere_last_error().is_null());
        // The guard frees this allocation before the next independent load.
    }
    // SAFETY: the documented free operation accepts null.
    unsafe { rere_free_csr(ptr::null_mut()) };
    assert!(rere_last_error().is_null());
}

#[test]
fn ffi_empty_graph_has_one_zero_offset_and_can_be_freed() {
    let fixture = TempFile::new(b"8 8\n# no usable edges\n");
    let path = fixture.c_path();
    // SAFETY: the owned CString remains valid for the entire load call.
    let owner = LoadedGraph(unsafe { rere_load_csr(path.as_ptr()) });
    assert_eq!(owner.snapshot(), graph(&[0], &[]));
    assert!(rere_last_error().is_null());
}

#[test]
fn ffi_errors_are_isolated_between_threads() {
    // SAFETY: null is explicitly checked by the loader.
    assert!(unsafe { rere_load_csr(ptr::null()) }.is_null());
    let parent_error = last_error_text().expect("parent thread error");
    let worker_error = std::thread::spawn(|| {
        assert!(rere_last_error().is_null());
        let invalid_utf8 = [0xFFu8, 0];
        // SAFETY: the bytes form a readable NUL-terminated string.
        assert!(unsafe { rere_load_csr(invalid_utf8.as_ptr().cast()) }.is_null());
        let error = last_error_text().expect("worker thread error");
        let fixture = TempFile::new(b"1 2\n");
        let path = fixture.c_path();
        // SAFETY: path is valid and owned until the load returns.
        let owner = LoadedGraph(unsafe { rere_load_csr(path.as_ptr()) });
        assert_eq!(owner.snapshot(), graph(&[0, 1, 2], &[1, 0]));
        assert!(rere_last_error().is_null());
        error
    })
    .join()
    .expect("worker completed");
    assert_ne!(worker_error, parent_error);
    assert_eq!(last_error_text().as_deref(), Some(parent_error.as_str()));
}
