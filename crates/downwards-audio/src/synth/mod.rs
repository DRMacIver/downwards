//! NES-flavoured synthesis primitives: two pulse shapes, a stepped triangle,
//! and a 15-bit LFSR noise source, plus click-free gain ramps and the output
//! conditioning chain.

pub mod mixer;
pub mod voice;
