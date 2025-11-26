use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FirestoreStringValue {
    #[serde(rename = "stringValue")]
    pub string_value: String,
}

impl Default for FirestoreStringValue {
    fn default() -> Self {
        Self {
            string_value: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FirestoreMapFields<T> {
    #[serde(default)]
    pub fields: HashMap<String, T>,
}

impl<T> Default for FirestoreMapFields<T> {
    fn default() -> Self {
        Self {
            fields: HashMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FirestoreMapValue<T: Default> {
    #[serde(rename = "mapValue")]
    pub map_value: FirestoreMapFields<T>,
}

impl<T: Default> Default for FirestoreMapValue<T> {
    fn default() -> Self {
        Self {
            map_value: FirestoreMapFields::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct FirestoreRoomFields {
    #[serde(default)]
    pub offers: FirestoreMapValue<FirestoreStringValue>,
    #[serde(default)]
    pub answers: FirestoreMapValue<FirestoreStringValue>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FirestoreRoomDoc {
    #[serde(default)]
    pub fields: FirestoreRoomFields,
}

// For patching offers/answers, we just need to match the structure
#[derive(Serialize, Debug)]
pub struct FirestorePatchOffers {
    pub fields: FirestoreOffersField,
}

#[derive(Serialize, Debug)]
pub struct FirestoreOffersField {
    pub offers: FirestoreMapValue<FirestoreStringValue>,
}

#[derive(Serialize, Debug)]
pub struct FirestorePatchAnswers {
    pub fields: FirestoreAnswersField,
}

#[derive(Serialize, Debug)]
pub struct FirestoreAnswersField {
    pub answers: FirestoreMapValue<FirestoreStringValue>,
}

#[derive(Debug, Clone)]
pub enum FirestoreInboxMessage {
    Doc(FirestoreRoomDoc),
    RoomCreated,
    RoomNotFound,
}
