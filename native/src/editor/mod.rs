pub mod buffer;
pub mod highlight;
pub mod layout_cache;
pub mod layout_tree;
pub mod renderer;

#[cfg(test)]
mod render_snapshot_tests;

#[cfg(test)]
pub(crate) mod test_docs;

#[cfg(test)]
mod render_preview;
