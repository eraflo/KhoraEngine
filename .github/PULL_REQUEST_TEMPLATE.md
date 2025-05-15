## Description

Please include a summary of the change and which issue is fixed or feature is implemented. Please also include relevant motivation and context. List any dependencies that are required for this change.

*   Closes # (issue number if it fixes a bug)
*   Implements # (issue number if it adds a feature discussed in an issue)

## Type of change

Please check the boxes that apply or delete options that are not relevant.

- [ ] **Bug fix** (non-breaking change which fixes an issue)
- [ ] **New feature** (non-breaking change which adds functionality)
- [ ] **Breaking change** (fix or feature that would cause existing functionality to not work as expected)
- [ ] **Documentation update**
- [ ] **Refactoring / Performance improvements**
- [ ] **Build system / CI improvements**
- [ ] **Other** (please describe):

## How Has This Been Tested?

Please describe the tests that you ran to verify your changes. Provide instructions so we can reproduce, if applicable. Please also list any relevant details for your test configuration.

- [ ] New unit tests added/updated for the changes.
- [ ] All existing unit tests pass with these changes.
- [ ] Manual testing performed as described below:
    *   Step 1: ...
    *   Step 2: ...
    *   Observed result: ...

**Test Configuration (if relevant for manual testing)**:
*   KhoraEngine Version/Commit:
*   OS:
*   Graphics Backend:

## Checklist:

Before submitting your pull request, please make sure you have completed the following:

- [ ] I have read the [CONTRIBUTING.md](https://github.com/eraflo/KhoraEngine/blob/main/CONTRIBUTING.md) file (if it exists and is relevant for this type of contribution).
- [ ] My code follows the style guidelines of this project. I have run `cargo fmt --all -- --check` locally and it passes.
- [ ] My code has been linted with `cargo clippy --workspace --all-targets --all-features -- -D warnings` and there are no new clippy warnings.
- [ ] I have performed a self-review of my own code.
- [ ] I have commented my code, particularly in hard-to-understand areas or for complex logic.
- [ ] I have made corresponding changes to the documentation (if applicable).
- [ ] My changes generate no new compiler warnings.
- [ ] I have added tests that prove my fix is effective or that my feature works (if applicable).
- [ ] New and existing unit tests pass locally with my changes (`cargo test --workspace --all-targets --all-features`).
- [ ] Any dependent changes have been merged and published in downstream modules (if applicable).

## Screenshots (if applicable)
[If your change has a visual component, please add screenshots here showing the before/after or the new UI/feature.]

## Further comments or questions for the reviewer
[Add any other comments, questions, or specific points you'd like the reviewer (currently @eraflo) to focus on.]