//! The link roots of a payload: the entry functions its profile in the
//! builder names, and the start-up hook. The patched sites of the original
//! reach them by address and nothing in the payload calls them, so a table
//! of them in `.patch.link_roots`, which every linker script keeps, keeps
//! them in the link. Each patch's `link.rs` holds that table
//! (`link_roots!`) and nothing else but its panic handler.

/// The address of a function the link keeps.
pub struct Root(pub *const ());
// SAFETY: the table is only read by the linker; nothing dereferences it.
unsafe impl Sync for Root {}

/// The table of link roots, one function each.
#[macro_export]
macro_rules! link_roots {
    ($($entry:path),* $(,)?) => {
        #[used]
        #[link_section = ".patch.link_roots"]
        static LINK_ROOTS: [$crate::link::Root; [$(stringify!($entry)),*].len()] =
            [$($crate::link::Root($entry as *const ())),*];
    };
}
