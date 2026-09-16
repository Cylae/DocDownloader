# DocDownloader

> High-performance, reliable document acquisition and offline reconstruction engine written in modern Rust.

DocDownloader accepts a supported interactive publication/reader URL and, when the publication is legitimately accessible through the public reader, automatically resolves metadata, discovers ordered page manifests, selects the highest legitimately available resolution, downloads pages with bounded concurrency and resume capabilities, and reconstructs verified, atomic offline PDF documents.

---

## Features

- **Provider-Independent Architecture**: Decoupled domain models and extraction contracts (`PublicationProvider`) capable of supporting multiple publication platforms.
- **Production Calaméo Extraction**: Native parser for Calaméo book JSON metadata and fallback HTML reader extraction, handling URL query parameters, access signatures, and multi-tier resolution selection.
- **Direct PDF Optimization**: Automatically detects and verifies publisher-provided direct PDF downloads when publicly exposed.
- **Highest Legitimate Quality**: Deterministic resolution selection favoring original/uncompressed assets over lower reader resolutions without artificial upscaling.
- **Bounded Concurrency & Streaming**: Configurable worker pool (default: 4, bounded 1–16) streaming page assets directly to disk cache. RAM consumption remains constant regardless of document page count.
- **Resumable Downloads & Checkpointing**: Checkpointed `job.json` and page-level SHA-256 integrity verification allowing interrupted downloads to resume seamlessly without re-fetching valid assets.
- **Atomic Output & PDF Validation**: Atomic file writes (`.part` staging) guarantee no half-written or corrupted PDFs are exposed. Final documents undergo programmatic PDF structure and page-count verification via `lopdf`.
- **Enterprise-Grade Network Hardening**: Custom `SecureDnsResolver` preventing SSRF (blocking loopback, RFC1918 private subnets, cloud metadata `169.254.169.254`), re-validating redirect hops, and parsing `Retry-After` on HTTP 429.
- **Dual Interface**: Full-featured CLI and an optional lightweight local web application bound securely to `127.0.0.1`.

---

## Provider Support Matrix

| Provider | URL Detection | Metadata Extraction | Page Manifest | Best Quality Selection | Resume & Cache | Direct PDF Optimization | Status |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **Calaméo** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ (when public) | **Stable** |
| **Issuu** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ (when public) | **Stable** |
| **SlideShare** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ (when public) | **Stable** |
| **Scribd** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ (when public) | **Stable** |

---

## Supported URL Examples

DocDownloader supports standard, embed, and query variants across all supported publication platforms:

- **Calaméo**:
  - `https://www.calameo.com/read/0061133461a5012e8961a`
  - `https://www.calameo.com/books/0061133461a5012e8961a`
- **Issuu**:
  - `https://issuu.com/username/docs/magazine_slug`
  - `https://e.issuu.com/embed.html?d=magazine_slug&u=username`
- **SlideShare**:
  - `https://www.slideshare.net/author/presentation-slug`
  - `https://www.slideshare.net/slideshow/embed_code/key/abcdef123`
- **Scribd**:
  - `https://www.scribd.com/document/123456789/Title-Slug`
  - `https://www.scribd.com/embeds/123456789/content?start_page=1&view_mode=scroll`

---

## Installation & Building

### Prerequisites
- Modern Rust toolchain (Rust 2024 edition, 1.85+)
- On Windows: standard MinGW or MSVC development toolchains

### Building from Source
```bash
git clone https://github.com/Cylae/DocDownloader.git
cd DocDownloader

# Build release binary
cargo build --release

# The compiled binary will be located at:
# target/release/docdownloader (Linux/macOS)
# target/release/docdownloader.exe (Windows)
```

---

## Usage

### 1. Download Command
Download and assemble a publication into an offline PDF:

```bash
# Shorthand invocation
docdownloader "https://www.calameo.com/read/0061133461a5012e8961a"

# Explicit subcommand with custom destination and concurrency
docdownloader download "https://www.calameo.com/read/0061133461a5012e8961a" \
  --output "reports/annual_report.pdf" \
  --concurrency 6 \
  --verbose
```

#### Download Options
- `-o, --output <PATH>`: Custom output file path.
- `--output-dir <DIR>`: Custom target directory for the generated PDF.
- `-c, --concurrency <N>`: Worker thread concurrency between 1 and 16 (default: 4).
- `--timeout <SECS>`: Network request timeout in seconds (default: 30).
- `--retries <N>`: Maximum retry attempts for transient network errors (default: 3).
- `--no-resume`: Disable cache resumption and re-download all pages.
- `-f, --force`: Overwrite existing output file if it already exists.
- `-q, --quiet`: Suppress interactive progress bars.
- `-v, --verbose`: Enable diagnostic trace logging.
- `--diagnostic <PATH>`: Export sanitized diagnostic JSON bundle on completion or failure.

### 2. Inspect Command
Resolve metadata, total page count, candidate asset dimensions, and page quality breakdown without downloading pages:

```bash
docdownloader inspect "https://www.calameo.com/read/0061133461a5012e8961a"
```

*Example Output:*
```text
Provider: calameo
Title: Le Condensé N°4
Author/Publisher: Saint Joseph Lannion
Publication ID: 0061133461a5012e8961a
Pages: 2
Document Geometry: 595x842 pt
Best Discovered Quality: 595x842 px (ImageJpeg)
Direct PDF: Unavailable
Extraction Method: High-resolution page assets
Thumbnail: http://i.calameoassets.com/211022160601-3b723dd9df70eeb8937f6e31fa1d3668/p1.jpg

Page Quality Breakdown:
  1–2       595×842  ImageJpeg
```

### 3. Batch Mode
Batch download and reconstruct multiple publications from a URL list file (supports `#` comments):

```bash
docdownloader batch publications.txt --concurrency 6 --output-dir ./downloads
```

### 4. Diagnostic Bundle Export
Generate a sanitized JSON diagnostic report (Directive 60) containing tool version, provider, sanitized URL (with all authentication cookies, tokens, and credentials stripped), stage reached, and HTTP status history:

```bash
# Print to stdout
docdownloader diagnostic "https://www.calameo.com/read/0061133461a5012e8961a"

# Export to file
docdownloader diagnostic "https://www.calameo.com/read/0061133461a5012e8961a" -o diag.json
```

### 5. Cache Management
View or purge temporary publication caches:

```bash
# View cache usage
docdownloader cache status

# Clean expired or all temporary caches
docdownloader cache clean --all
```

### 6. Local Web UI
Launch an embedded, privacy-focused Web UI running locally:

```bash
docdownloader serve --port 8080
```
Then open `http://127.0.0.1:8080` in your web browser.
- Bound strictly to `127.0.0.1` by default for local privacy and security.
- Real-time Server-Sent Events (SSE) streaming download progress.
- Clean, semantic HTML5/CSS interface with responsive dark mode.

### 7. Process Exit Codes

| Exit Code | Classification | Description |
| :---: | :--- | :--- |
| `0` | **Success** | Operation completed successfully with verified output |
| `1` | **InvalidInput** | Invalid URL syntax or rejected host/SSRF redirect |
| `2` | **UnsupportedProvider** | URL does not match any registered provider |
| `3` | **AccessRestricted** | Publication not found or restricted/private |
| `4` | **TransientOrNetwork** | Network timeout, rate limit, or invalid metadata/manifest |
| `5` | **CorruptData** | Corrupted page content or structural PDF validation failure |
| `6` | **FileSystem** | Destination file exists or disk I/O / permission error |
| `7` | **PdfBuild** | Fatal PDF stream generation failure |
| `8` | **InvariantViolation** | Internal system invariant violation |
| `130` | **Cancelled** | Operation cancelled by user (SIGINT / Ctrl+C) |

---

## Architecture

DocDownloader enforces strict separation between networking, provider extraction, download scheduling, and document assembly:

```text
src/
├── main.rs                  # CLI entry point (clap v4)
├── lib.rs                   # Library interface and public API
├── cli/                     # CLI argument parsing, commands, and progress rendering
├── core/
│   ├── diagnostic.rs        # Sanitized diagnostic bundle generation (Directive 60)
│   ├── document.rs          # Publication & PageDescriptor domain models
│   ├── engine.rs            # Concurrent bounded download & assembly engine
│   ├── error.rs             # Structured DocDownloaderError taxonomy
│   ├── job.rs               # JobState lifecycle and progress event models
│   └── quality.rs           # Multi-tier page quality reporting (Directive 65)
├── network/
│   ├── client.rs            # Hardened Reqwest client (redirects, stream limits)
│   ├── retry.rs             # Exponential backoff, jitter, and Retry-After parsing
│   └── security.rs          # SecureDnsResolver blocking private IP ranges / SSRF
├── pdf/
│   ├── builder.rs           # Memory-bounded PDF generator (lopdf)
│   ├── image.rs             # Image validation and dimension sanitization
│   └── validator.rs         # Post-generation PDF structural validation
├── providers/
│   ├── mod.rs               # PublicationProvider trait abstraction
│   └── calameo/             # Calaméo API, book.json parser, signature generator
├── storage/
│   ├── atomic.rs            # Atomic file creation (.part rename)
│   ├── cache.rs             # Page cache storage, integrity hashing, job state
│   └── sanitize.rs          # Cross-platform filename and path confinement
└── web/
    ├── server.rs            # Axum web server and router (64KB body limit)
    └── handlers.rs          # REST and SSE endpoints with path redaction
```

---

## Correctness & Quality Principles

1. **Lossless / Native Image Embedding**: JPEGs are embedded directly into PDF XObject streams without decoding and lossy recompression, preserving byte-exact quality and maximizing assembly speed.
2. **Aspect Ratio Preservation**: Each PDF page dictionary defines exact media box dimensions matching the source image dimensions.
3. **Deterministic Page Ordering**: Pages are sorted and verified by 1-based logical index; any missing or non-contiguous page immediately halts final PDF assembly.
4. **Resilience to Transient Errors**: Exponential backoff retries transient 5xx, timeouts, and connection drops, honoring `Retry-After` headers on HTTP 429.
5. **Atomic Filesystem Guarantees**: Interrupted downloads leave a recoverable cache; interrupted PDF generation never leaves a corrupt `.pdf` in the output path.

---

## Security Model

- **SSRF & DNS Rebinding Protection**: URL resolution evaluates IP addresses against private subnets (RFC 1918 `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`), loopback (`127.0.0.0/8`, `::1`), link-local (`169.254.0.0/16`), and broadcast/multicast ranges. Redirect hops are re-validated before being followed.
- **Decompression Bomb Defense**: Decoded images are strictly limited to `8192 × 8192` pixels; payloads over 50MB per page are aborted immediately.
- **Path Traversal Sanitization**: Metadata strings and filenames are sanitized against directory traversal (`../`, `..\`), forbidden Windows device names (`CON`, `PRN`, `AUX`, `NUL`), control characters, and reserved symbols.
- **Local Web Isolation**: The web server binds exclusively to `127.0.0.1` by default and enforces a 64KB maximum body limit.

---

## Performance Benchmarks

DocDownloader processes publications with linear $O(N)$ time complexity and constant $O(1)$ memory consumption by streaming assets directly to disk:

| Publication Size | Assembly Latency | Generated PDF Size | Throughput |
| :---: | :---: | :---: | :---: |
| **10 Pages** | 23.6 ms | 14.4 KB | **422.7 pages/sec** |
| **100 Pages** | 427.3 ms | 141.5 KB | **234.0 pages/sec** |
| **500 Pages** | 653.4 ms | 707.7 KB | **765.2 pages/sec** |

*Measured on standard workstation hardware via `cargo bench --bench synthetic_bench`.*

---

## Verification & Testing

DocDownloader comes with a complete suite of unit tests, integration tests with synthetic provider fixtures, property-based tests via `proptest`, and adversarial security tests.

### Running Unified Verification
```bash
# On Linux / macOS:
./verify.sh

# On Windows (PowerShell):
.\verify.ps1
```

The verification suite executes:
1. `cargo fmt --check` (Zero formatting violations)
2. `cargo clippy --all-targets --all-features -- -D warnings` (Zero compiler or lint warnings)
3. `cargo test --all-targets --all-features` (48 unit, integration, WireMock, security, regression, resume, and property tests)
4. `cargo bench --bench synthetic_bench` (10, 100, and 500-page performance validation)
5. `cargo build --release` (Optimized binary verification)

---

## Legitimate Access Boundary & Legal Notice

DocDownloader is designed solely for legitimate offline access to publicly visible publications.
- **No DRM or Paywall Bypass**: Does not defeat authentication, passwords, subscriptions, or DRM access controls.
- **Honors Access Restrictions**: If a provider indicates that a publication is private, password-protected, or restricted, the application immediately aborts with an explicit `AccessRestricted` error.
- **Respects Platform Infrastructure**: Uses conservative, bounded concurrency and strictly respects HTTP 429 throttling.

---

## License

DocDownloader is released under the [MIT License](LICENSE).
