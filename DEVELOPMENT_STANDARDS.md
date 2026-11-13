# Nanomite Development Standard (Multi-Language)

**Mandatory daily coding standard for all nanomite project code.**  
Applies to every language in the stack unless explicitly overridden by the user.

## Universal Principles (All Languages)

| Principle | Requirement |
|-----------|-------------|
| **Immutability** | Prefer frozen / readonly / final data structures. Avoid mutable shared state. |
| **Lazy evaluation** | Use generators, iterators, streams, or lazy sequences; never materialize large collections unless required. |
| **Resource lifecycle** | Use context managers, `defer`, `try-with-resources`, `using`, RAII, or equivalent deterministic cleanup. |
| **Type safety** | Enable the strictest static type system available for the language (strict mode, gradual typing, or nominal types). |
| **Fail-fast** | Validate inputs at boundaries; raise domain-specific exceptions immediately; never silently swallow errors. |
| **Observability** | Use structured logging; never `print` / `console.log` / `fmt.Print` in library code for diagnostics. |
| **Pure / I/O separation** | Keep business logic pure and side-effect free; isolate I/O (network, disk, CLI) in thin adapters. |
| **Testability** | Every module ships with unit tests; aim for deterministic, fast, dependency-injected tests. |
| **FOSS tooling** | Use only free/open-source compilers, linters, formatters, test runners, and package managers. |
| **CLI discipline** | If a binary exposes a CLI, use a parsing library with explicit exit codes (0 = success, non-zero = failure). |

---

## Language-Specific Mapping

### Python
- Data: `@dataclass(frozen=True, slots=True)`
- Lazy: generators (`yield`), `itertools`
- Resources: `contextlib.contextmanager`, `with` statement
- Types: `from __future__ import annotations`, `typing`, mypy strict
- Errors: custom exception hierarchy (`BaseError` → domain errors)
- Logging: `logging`, structured via `structlog` or `logging.LoggerAdapter`
- Tests: `pytest`
- Lint/Format: `ruff`, `mypy`
- CLI: `argparse`, `sys.exit(code)`

### TypeScript / JavaScript
- Data: `readonly` properties, `as const`, frozen objects (`Object.freeze`), immutable records (TC39 proposal or Immutable.js patterns)
- Lazy: generators (`function*`), iterators, lazy streams
- Resources: `try/finally`, `Symbol.dispose` / `using` (TS 5.2+), explicit close methods
- Types: TypeScript `strict: true`, `noImplicitAny`, `exactOptionalPropertyTypes`
- Errors: custom `Error` subclasses with `code` and `context` fields
- Logging: `pino` or `winston` structured JSON logs
- Tests: `vitest` or `jest`
- Lint/Format: `eslint`, `prettier`, `tsc --noEmit`
- CLI: `yargs` or `commander`, `process.exitCode`

### Rust
- Data: `struct` with private fields, const references, avoid `Cell`/`RefCell` unless necessary
- Lazy: `Iterator` trait, lazy adapters (`map`, `filter`, `flat_map`)
- Resources: `Drop` trait, RAII (`Box`, `Rc`, `Arc`, `MutexGuard`), `std::fs::File` scoping
- Types: strong static typing, `Result<T, E>`, `Option<T>`, no unwraps in library code
- Errors: `thiserror` / `anyhow` domain error enums
- Logging: `tracing` + `tracing-subscriber`
- Tests: built-in `#[test]`, `cargo test`
- Lint/Format: `clippy`, `rustfmt`
- CLI: `clap`, `std::process::exit(code)`

### Go
- Data: unexported fields, immutable by convention, avoid pointer mutation of arguments
- Lazy: channels, generator functions returning `<-chan T`, `iter.Seq` (Go 1.23+)
- Resources: `defer`, `io.Closer`, `sync.Once` for singletons
- Types: static typing, custom error types with `Is`/`As`, no blank `_ = err`
- Errors: sentinel errors, wrapped errors (`fmt.Errorf("...: %w", err)`)
- Logging: `log/slog` structured logging
- Tests: built-in `testing` package, `go test`
- Lint/Format: `golangci-lint`, `gofmt`
- CLI: `cobra` or `urfave/cli`, `os.Exit(code)`

### C++
- Data: `const` member functions, `const` objects, POD or value semantics
- Lazy: `std::ranges::views`, coroutines (`co_yield`), input iterators
- Resources: RAII (`std::unique_ptr`, `std::shared_ptr`, `std::lock_guard`)
- Types: templates/concepts, `std::optional`, `std::variant`, `std::expected` (C++23)
- Errors: custom `std::exception` subclasses, error codes or `std::expected`
- Logging: `spdlog`
- Tests: `Catch2` or `GoogleTest`
- Lint/Format: `clang-tidy`, `clang-format`
- CLI: `CLI11`, `std::exit(code)`

### Java
- Data: `record`, `final` classes, immutable collections (`List.of`, `Map.of`)
- Lazy: `Stream`, lazy collectors, custom `Iterator`
- Resources: `try-with-resources`, `AutoCloseable`
- Types: strong nominal typing, `Optional`, sealed classes/interfaces
- Errors: checked exceptions where meaningful, custom `Exception` hierarchy
- Logging: `SLF4J` + `Logback` or `Log4j2`
- Tests: `JUnit 5`
- Lint/Format: `checkstyle`, `spotbugs`
- CLI: `picocli` or `Apache Commons CLI`, `System.exit(code)`

### C# (.NET)
- Data: `record`, `init`-only properties, immutable collections (`System.Collections.Immutable`)
- Lazy: `IEnumerable<T>`, `yield return`, `IAsyncEnumerable`
- Resources: `using`, `IDisposable`, `IAsyncDisposable`
- Types: strong static typing, nullable reference types enabled
- Errors: custom `Exception` classes
- Logging: `Serilog` or `Microsoft.Extensions.Logging`
- Tests: `xUnit` or `NUnit`
- Lint/Format: Roslyn analyzers, `dotnet format`
- CLI: `System.CommandLine`, `Environment.Exit(code)`

---

## Project Skeleton (Language-Agnostic)

```
project/
├── README.md
├── LICENSE (NANOMITE Proprietary License)
├── .github/workflows/ci.yml
├── src/
│   └── <module>/
│       ├── __init__.py / mod.rs / index.ts / main.go / ...
│       ├── errors.py / error.rs / errors.ts / errors.go / ...
│       ├── domain.py / domain.rs / ...     (pure logic)
│       ├── io.py / io.rs / ...             (I/O adapters)
│       └── cli.py / cli.rs / ...           (entry point)
├── tests/
│   └── test_<module>.<ext>
├── pyproject.toml / Cargo.toml / package.json / go.mod / .csproj
└── DEVELOPMENT_STANDARDS.md
```

---

## CI Gate (All Languages)

Every change must pass before merge:

1. Format check (`ruff format --check`, `rustfmt --check`, `prettier --check`, `gofmt -l`, `clang-format --dry-run`)
2. Lint (`ruff check`, `clippy -D warnings`, `eslint`, `golangci-lint run`, `clang-tidy`)
3. Type check (`mypy --strict`, `tsc --noEmit`, `cargo check`, `javac -Xlint`, `dotnet build`)
4. Test suite (`pytest`, `cargo test`, `vitest run`, `go test ./...`, `mvn test`, `dotnet test`)
5. No `print` / `console.log` in library code (grep guard)

---

## Exit Code Contract

```
0   Success
1   Generic error
2   Misuse of shell / CLI (bad args)
64  Command line usage error (sysexits)
65  Data format error
66  Cannot open input
67  Address unknown
68  Host unknown
69  Service unavailable
70  Internal software error
71  System error (OS failure)
72  Critical OS file missing
73  Can't create output file
74  Input/output error
75  Temporary failure
76  Remote error in protocol
77  Permission denied
78  Configuration error
```

---

## Enforcement

This file is the mandatory standard. Any new code generated for nanomite projects must conform to the relevant language section. Existing legacy code should be refactored on touch (boy-scout rule).
