//! pm's customization layer over [`gpui_component`] — wrappers, extensions and
//! variants of kit components, plus the few primitives the kit doesn't provide.
//!
//! Dependency direction is `pm-ui → fremantle → gpui-component`. Anything pm
//! restyles or preconfigures lives here rather than inline at the call site, so
//! there is one seam between what the kit gives us and how pm uses it. pm-ui
//! reaches for `gpui_component` directly only where a component is used
//! completely unmodified.
//!
//! Note this is deliberately *not* the domain-agnostic primitives library the
//! crate started as (PM-55): building those from scratch was superseded by
//! adopting gpui-component, which already ships them.

pub mod decorations;
pub mod scroll;
pub mod text_input;
pub mod theme;
