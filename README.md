# rere

Read an edge list file as an UNDIRECTED SIMPLE graph and convert it into CSR.

Compatible with C/C++.

Good performance.

## Build

Rust >= 1.85.0

```sh
cargo build --release
```

## Input

Example:

```text
% Lines that do not contain a valid edge
% are ignored.
42 10
10 42
7 42
99 99
```

## API

```cpp
auto rere_load_csr(char const* path) -> Csr*;
void rere_free_csr(Csr* csr);
auto rere_last_error(void) -> char const*;
```

Upon successful loading, the function returns a pointer to the graph (transferring ownership) and clears the current thread's error state; `rere_last_error()` will subsequently return `NULL`. If loading fails, it returns `NULL`, and `rere_last_error()` can be called immediately to retrieve the error string for the current thread. The string is managed by the library and remains valid until the same thread next calls any loading function or the thread terminates—whichever occurs first; if the string needs to be retained, copy it, and do not modify or free it.

The graph structure and its two arrays are read-only and remain valid until `rere_free_csr()` is called. The raw pointer returned by the loading function must be freed exactly once; do not pass a copy of the structure, do not modify its fields or arrays, and do not use `free()` or `delete` to deallocate it. Once freed, any borrowed pointers referencing the graph or its arrays become invalid. Calling `rere_free_csr(NULL)` is permissible.

## Usage

```cpp
#include "rere.h"
#include <iostream>
#include <memory>

int main() {
    using Graph = std::unique_ptr<Csr, decltype(&rere_free_csr)>;
    Graph graph(rere_load_csr("zachary.txt"), &rere_free_csr);
    if (!graph) {
        auto error = rere_last_error();
        std::cerr << "load failed: " << (error ? error : "unknown error") << '\n';
        return 1;
    }

    std::cout << "vertices=" << graph->vertices_count
              << ", edges=" << graph->edges_count << '\n';
    return 0;
}
```

## Link

Which native libraries are required for static linking?

```sh
cargo rustc --release -- --print native-static-libs
```

See `native-static-libs:`.

