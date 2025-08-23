# Contributing to KhoraEngine

First off, thank you for considering contributing to KhoraEngine! It's an ambitious project, and every contribution, from a bug report to a documentation fix, is greatly appreciated.

KhoraEngine is currently in its early stages of development. We are primarily focused on building the core Symbiotic Adaptive Architecture (SAA) and its foundational components.

## Development Prerequisites

Before contributing, please ensure you have the required tooling installed.

### Documentation (`mdBook`)

Our narrative documentation is built using `mdBook`. To preview your documentation changes locally, you will need to install it:

```bash
# Install mdBook via cargo
cargo install mdbook
```

Once installed, you can serve the book locally from the project root:
```bash
# This command builds the book, starts a local web server, and opens it in your browser.
mdbook serve docs --open
```

## How Can I Contribute?

*   **Join the Discussion:** Share your thoughts, ideas, and feedback on our [GitHub Discussions page](https://github.com/eraflo/KhoraEngine/discussions).
*   **Report Bugs:** If you experiment with the engine and find bugs, please open an [Issue](https://github.com/eraflo/KhoraEngine/issues) using the "Bug Report" template, providing detailed steps to reproduce.
*   **Suggest Features:** Have ideas for features that would complement the SAA vision? Propose them as an [Issue](https://github.com/eraflo/KhoraEngine/issues) using the "Feature Request" template.
*   **Improve Documentation:** If you see areas for improvement or clarification in our documentation book, pull requests are welcome!

## Code Contributions

Direct code contributions are welcome, but given the architectural nature of the current work, please start by opening an Issue or a Discussion to talk about your proposed changes with the maintainer (@eraflo) first. This ensures that your work aligns with the project's roadmap and architectural principles.

### Pull Request Process

1.  Ensure any new code adheres to our coding standards by running `cargo xtask all`.
2.  Update the documentation (`mdBook` for concepts, `rustdoc` comments for API) with any relevant changes.
3.  Open a pull request and link it to the relevant issue.

## Code of Conduct

By participating in this project, you are expected to uphold our [Code of Conduct](CODE_OF_CONDUCT.md).

We look forward to building this innovative engine with you!