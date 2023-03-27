pub mod html;
pub mod md;

pub use html::*;
pub use md::*;

#[derive(PartialEq)]
pub enum WriteTo {
	Beginning,
	End,
}
