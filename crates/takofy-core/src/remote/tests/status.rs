use super::super::*;

use librespot::protocol::entity_extension_data::{EntityExtensionData, EntityExtensionDataHeader};
use librespot::protocol::extended_metadata::{
    BatchedExtensionResponse, EntityExtensionDataArray, EntityExtensionDataArrayHeader,
};
use protobuf::MessageField;

/// One batched answer: an array with `provider` and the `(uri, code)`
/// entities, where `None` leaves the entity's header absent.
fn status_response(provider: i32, entities: &[(&str, Option<i32>)]) -> BatchedExtensionResponse {
    let mut array = EntityExtensionDataArray {
        header: MessageField::some(EntityExtensionDataArrayHeader {
            provider_error_status: provider,
            ..Default::default()
        }),
        ..Default::default()
    };
    for (uri, code) in entities {
        let header = match code {
            Some(code) => MessageField::some(EntityExtensionDataHeader {
                status_code: *code,
                ..Default::default()
            }),
            None => MessageField::none(),
        };
        array.extension_data.push(EntityExtensionData {
            header,
            entity_uri: (*uri).to_owned(),
            ..Default::default()
        });
    }
    BatchedExtensionResponse {
        extended_metadata: vec![array],
        ..Default::default()
    }
}

#[test]
fn a_two_hundred_array_records_nothing() {
    let response = status_response(200, &[("spotify:track:a", Some(200))]);
    assert_eq!(tally_of(&response).summary(), None);
}

#[test]
fn a_failing_provider_code_is_recorded() {
    let response = status_response(500, &[("spotify:track:a", Some(200))]);
    assert_eq!(
        tally_of(&response).summary().as_deref(),
        Some("metadata statuses: provider 500 x1")
    );
}

#[test]
fn two_failing_entities_are_two_uris() {
    let response = status_response(
        200,
        &[
            ("spotify:track:a", Some(404)),
            ("spotify:track:b", Some(404)),
        ],
    );
    assert_eq!(
        tally_of(&response).summary().as_deref(),
        Some("metadata statuses: entities 404 x2")
    );
}

#[test]
fn one_uri_twice_is_one_entity() {
    let response = status_response(
        200,
        &[
            ("spotify:track:a", Some(404)),
            ("spotify:track:a", Some(404)),
        ],
    );
    assert_eq!(
        tally_of(&response).summary().as_deref(),
        Some("metadata statuses: entities 404 x1")
    );
}

#[test]
fn an_entity_with_no_header_records_nothing() {
    let response = status_response(200, &[("spotify:track:a", None)]);
    assert_eq!(tally_of(&response).summary(), None);
}
