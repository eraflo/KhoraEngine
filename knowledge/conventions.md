# Conventions

## Naming

- **Crates**: `khora-{name}` (kebab-case)
- **Modules**: `snake_case`
- **Types/Traits**: `PascalCase` (e.g., `LaneContext`, `WgpuDevice`)
- **Functions/Methods**: `snake_case`
- **Constants**: `SCREAMING_SNAKE_CASE`
- **GPU Resources**: suffixed with `Id` (e.g., `TextureId`, `BufferId`)
- **Lanes**: suffixed with `Lane` (e.g., `LitForwardLane`, `ShadowPassLane`)
- **Agents**: suffixed with `Agent` (e.g., `RenderAgent`, `UiAgent`)

## Code Patterns

- `Arc<dyn Trait>` for cross-crate shared references
- `Arc<Mutex<T>>` for shared mutable state (e.g., `WgpuGraphicsContext`)
- `Result<T, SpecificError>` — never `unwrap()` on GPU/IO operations
- `log::{info, warn, error}` — never `println!`
- `// SAFETY:` comment required on every `unsafe` block
- `#[cfg(test)] mod tests` in every module with public functions

## File Layout

- One primary type per file (e.g., `device.rs` → `WgpuDevice`)
- Tests in `#[cfg(test)]` module at bottom of file
- Integration tests in `crates/{name}/tests/`
- Benchmarks in `crates/{name}/benches/`

## Git

- Development on `dev` branch
- Stable releases on `main`
- CI: GitHub Actions (`rust.yml`)
- Pre-commit: `cargo xtask all` (fmt + clippy + test + doc)

## Documentation

- mdbook source in `docs/src/*.md`
- Build: `mdbook build docs/`
- Custom Ayu-dark theme in `docs/theme/custom.css`
