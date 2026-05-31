## Summary

- 

## Checks

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Risk

- [ ] No unrelated formatting churn
- [ ] Document mutations go through `EditorCommand` unless loading a file
- [ ] Markdown parsing stays outside renderer
- [ ] Renderer work stays proportional to visible content
- [ ] PDF page numbers clearly distinguish 0-based indexes from 1-based labels

## Notes

- 
