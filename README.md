# QUIC Connection Migration Scanner

This tool is designed to perform QUIC scans for servers to determine if they support the connection migration mechanism. It is based on the [Cloudflare Quiche](https://github.com/cloudflare/quiche) client.

Each scan provides information about:
- The success of the QUIC handshake
- The success of the connection migration
- The HTTP server used (when applicable)
- Eventual errors

## Build

The scanner is coded in Rust. It can be built using the Cargo package manager.

`cargo build --release`

By default this builds against the [quiche](https://github.com/cloudflare/quiche) release published on crates.io.
To build against a local quiche checkout instead (e.g. a patched fork), clone it into a `quiche` subfolder here and use
the provided build tool, which prefers that local checkout when present and otherwise falls back to crates.io:

```
git clone --recurse-submodules <quiche-repo-url> quiche
make build     # or `make release`
```

`make build`/`make release` just runs `./configure-quiche.sh` (which switches the `quiche` dependency in `Cargo.toml`
between the local path and the crates.io version depending on whether `./quiche` exists) followed by `cargo build`.

## Usage

The scanner takes as input a list of targets to scan. The list should be a text file with a header line, then one
`ip,domain` (or `ip,domain,port`) per line. The port column is optional and defaults to 443 when absent or empty.

There are also several options that can be used:
- -o --output FILE:             Write the results to the specified file (default: out.json)
- -w --wait-headers:            Wait until response headers are received before performing the migration
- -f --fast                     Shutdown the connection as soon as the response headers are received without waiting for the whole response
- --threads N                   Use N threads to perform the scans (default: 100)
- -m --migration-addr ADDRESS   Use the specified source address for the migration
- -a --address ADDRESS          Use the provide address as primary address.
- t --type TYPE                 Use the specified type of migration. Supported types are: 
    - standard (default): Perform a standard migration, using negotiated CID and path challenge
    - reuseCID: Perform a migration re-using the same CID and sending a path challenge
    - passive: Perform a passive migration, re-using the same CID and not sending a path challenge
- -p --multipath                Test multipath QUIC support. Advertises multipath support in the handshake
  (`initial_max_path_id` transport parameter) and reports whether the server negotiates it back
  (`multipath_supported` in the output). This only checks the handshake-level capability; it does not
  actively probe a second path. Requires a quiche build with multipath support (see Build).

 Example:
 ```
 ./migration_checker -o test_output.json -t passive -w --threads 50 test_input.txt
 ```