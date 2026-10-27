use super::{CoverError, MAX_COVER_BYTES, check_body, tail};
use takofy_model::CoverUrl;

fn url(value: &str) -> CoverUrl {
    CoverUrl::from_wire(value).expect("an allowlisted url")
}

#[test]
fn an_empty_body_is_rejected() {
    assert_eq!(check_body(&[], Some("image/jpeg")), Err(CoverError::Empty));
}

#[test]
fn an_oversized_body_is_rejected() {
    let body = vec![0; MAX_COVER_BYTES + 1];
    assert_eq!(check_body(&body, None), Err(CoverError::TooLarge));
}

#[test]
fn a_non_image_content_type_is_rejected() {
    assert_eq!(
        check_body(&[1], Some("text/html")),
        Err(CoverError::NotImage)
    );
}

#[test]
fn a_jpeg_body_passes() {
    assert_eq!(check_body(&[1], Some("image/jpeg")), Ok(()));
    assert_eq!(check_body(&[1], None), Ok(()));
}

#[test]
fn the_tail_is_the_end_of_the_url() {
    let url = url("https://i.scdn.co/image/0123456789abcdef");
    assert_eq!(tail(&url), "456789abcdef");
}
