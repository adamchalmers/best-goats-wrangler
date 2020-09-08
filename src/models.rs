use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize, Debug)]
pub struct Goat {
    // properties from database
    pub id: GoatId,
    pub name: String,
    pub image: String,
    #[serde(rename = "imageSmall")]
    pub image_small: String,
}

pub type GoatId = u32;
