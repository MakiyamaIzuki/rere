#ifndef RERE_H
#define RERE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Read-only CSR for an undirected, unweighted graph without self-loops.
 * Vertex IDs are reassigned in ascending order of the original IDs.
 * edges_count counts unique undirected edges, not adjacency entries.
 * row_offsets has vertices_count + 1 elements; column_indices has
 * row_offsets[vertices_count] == 2 * edges_count elements.
 * Neighbors in each row are sorted by their reassigned IDs.
 * Do not modify this struct or either array. Keep the original pointer
 * returned by rere_load_csr() and release it only with rere_free_csr().
 */
typedef struct Csr {
    uint64_t vertex_count;
    uint64_t edge_count;
    uint64_t* row_offsets;
    uint64_t* column_indices;
} Csr;

/* Load an edge-list file. A non-NULL path must point to a readable,
 * NUL-terminated string. NULL and non-UTF-8 paths are checked and return
 * NULL with a last-error message. Valid paths must use UTF-8.
 * Each line must contain exactly two ASCII decimal uint64 values,
 * separated by ASCII whitespace. Blank or invalid lines are skipped;
 * non-ASCII bytes, extra fields and overflow make a line invalid.
 * Self-loops are skipped, including their otherwise unseen vertices.
 * Duplicate and reversed edges are deduplicated. I/O errors fail the load.
 * Returns an owned graph on success (including an empty graph), or NULL
 * on failure. A successful call clears this thread's last error.
 */
Csr* rere_load_csr(char const* path);

/* Free a graph and both arrays. NULL is allowed. Otherwise csr must be
 * the original live pointer returned by a loader in this library, with
 * its fields and arrays unchanged. Do not use free()/delete, pass a copy
 * of the struct, or free the same graph twice. All borrowed pointers into
 * the graph become invalid after this call.
 */
void rere_free_csr(Csr* csr);

/* Borrow this thread's last load-error message; NULL if no error is set.
 * The NUL-terminated string remains valid only until this thread's next
 * load call or this thread exits, whichever
 * happens first. Do not modify or free it. Copy it before either event if
 * it must be kept longer.
 */
char const* rere_last_error(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RERE_H */
