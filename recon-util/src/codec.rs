use std::result::Result;

pub trait Codec {
    type Item;
    type EncodedItem;
    type Error;

    fn encode(&self, message: Self::Item) -> Result<Self::EncodedItem,Self::Error>;
    fn decode(&self, message: Self::EncodedItem) -> Result<Self::Item,Self::Error>;
}