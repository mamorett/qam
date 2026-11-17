use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Computes the lowercase hex-encoded SHA-256 digest of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex_lower(&hasher.finalize())
}

/// Encodes bytes as lowercase hexadecimal.
pub fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// URI-encodes `input` according to AWS S3 SigV4 rules.
///
/// Unreserved characters (`A-Z`, `a-z`, `0-9`, `-`, `_`, `.`, `~`) are left unchanged.
/// Spaces are encoded as `%20`. When `encode_slash` is false (for URI paths), `/` is kept as `/`.
pub fn uri_encode(input: &str, encode_slash: bool) -> String {
    const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        let is_unreserved = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if is_unreserved || (!encode_slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX_UPPER[(b >> 4) as usize] as char);
            out.push(HEX_UPPER[(b & 0x0f) as usize] as char);
        }
    }
    out
}

/// Builds a SigV4 canonical query string, sorted by encoded key then encoded value.
pub fn canonical_query_string(params: &[(&str, &str)]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let mut encoded: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| (uri_encode(k, true), uri_encode(v, true)))
        .collect();
    encoded.sort();
    encoded
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// Collapses runs of whitespace into a single ASCII space and trims leading/trailing whitespace.
fn normalize_header_value(val: &str) -> String {
    let mut out = String::with_capacity(val.len());
    let mut prev_space = true;
    for ch in val.chars() {
        if ch.is_ascii_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// Output of SigV4 request signing, including intermediate values for testing and debug logging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedRequest {
    pub canonical_request: String,
    pub string_to_sign: String,
    pub signature: String,
    pub authorization_header: String,
    pub amz_date: String,
    pub signed_headers: String,
}

/// Signs an HTTP request for S3 using AWS Signature Version 4.
///
/// `canonical_uri` must already be S3 URI-encoded (e.g. via [`uri_encode`] with `encode_slash = false`).
/// `headers` must include `host`, `x-amz-date`, `x-amz-content-sha256`, and any other signed headers.
#[allow(clippy::too_many_arguments)]
pub fn sign_request(
    method: &str,
    canonical_uri: &str,
    query_params: &[(&str, &str)],
    headers: &[(&str, &str)],
    payload_sha256_hex: &str,
    access_key_id: &str,
    secret_access_key: &str,
    region: &str,
    now: DateTime<Utc>,
) -> SignedRequest {
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let canonical_query = canonical_query_string(query_params);

    let mut norm_headers: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), normalize_header_value(v)))
        .collect();
    norm_headers.sort_by(|a, b| a.0.cmp(&b.0));

    // Merge duplicate header names if any appear.
    let mut merged: Vec<(String, String)> = Vec::with_capacity(norm_headers.len());
    for (k, v) in norm_headers {
        if let Some(last) = merged.last_mut()
            && last.0 == k
        {
            last.1.push(',');
            last.1.push_str(&v);
        } else {
            merged.push((k, v));
        }
    }

    let mut canonical_headers = String::new();
    let mut signed_header_names = Vec::with_capacity(merged.len());
    for (k, v) in &merged {
        canonical_headers.push_str(k);
        canonical_headers.push(':');
        canonical_headers.push_str(v);
        canonical_headers.push('\n');
        signed_header_names.push(k.as_str());
    }
    let signed_headers = signed_header_names.join(";");

    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_sha256_hex}"
    );

    let scope = format!("{date_stamp}/{region}/s3/aws4_request");
    let canonical_request_hash = sha256_hex(canonical_request.as_bytes());
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{canonical_request_hash}");

    let mut k_secret = Vec::with_capacity(4 + secret_access_key.len());
    k_secret.extend_from_slice(b"AWS4");
    k_secret.extend_from_slice(secret_access_key.as_bytes());

    let k_date = hmac_sha256(&k_secret, date_stamp.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    let k_signing = hmac_sha256(&k_service, b"aws4_request");

    let sig_bytes = hmac_sha256(&k_signing, string_to_sign.as_bytes());
    let signature = hex_lower(&sig_bytes);

    let authorization_header = format!(
        "AWS4-HMAC-SHA256 Credential={access_key_id}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    );

    SignedRequest {
        canonical_request,
        string_to_sign,
        signature,
        authorization_header,
        amz_date,
        signed_headers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const AWS_AKID: &str = "AKIAIOSFODNN7EXAMPLE";
    const AWS_SECRET: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";

    #[test]
    fn test_aws_sigv4_get_object_vector() {
        // Official AWS S3 SigV4 doc example: "Example: GET Object"
        let now = Utc.with_ymd_and_hms(2013, 5, 24, 0, 0, 0).unwrap();
        let empty_sha256 = sha256_hex(b"");
        assert_eq!(
            empty_sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let signed = sign_request(
            "GET",
            "/test.txt",
            &[],
            &[
                ("Host", "examplebucket.s3.amazonaws.com"),
                ("Range", "bytes=0-9"),
                ("x-amz-content-sha256", &empty_sha256),
                ("x-amz-date", "20130524T000000Z"),
            ],
            &empty_sha256,
            AWS_AKID,
            AWS_SECRET,
            "us-east-1",
            now,
        );

        let expected_canonical_request = "\
GET\n\
/test.txt\n\
\n\
host:examplebucket.s3.amazonaws.com\n\
range:bytes=0-9\n\
x-amz-content-sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n\
x-amz-date:20130524T000000Z\n\
\n\
host;range;x-amz-content-sha256;x-amz-date\n\
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

        assert_eq!(signed.canonical_request, expected_canonical_request);

        let expected_string_to_sign = "\
AWS4-HMAC-SHA256\n\
20130524T000000Z\n\
20130524/us-east-1/s3/aws4_request\n\
7344ae5b7ee6c3e7e6b0fe0640412a37625d1fbfff95c48bbb2dc43964946972";

        assert_eq!(signed.string_to_sign, expected_string_to_sign);
        assert_eq!(
            signed.signature,
            "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
        assert_eq!(
            signed.authorization_header,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    #[test]
    fn test_aws_sigv4_put_object_vector() {
        // Official AWS S3 SigV4 doc example: "Example: PUT Object" (test$file.text)
        let now = Utc.with_ymd_and_hms(2013, 5, 24, 0, 0, 0).unwrap();
        let payload_sha256 = sha256_hex(b"Welcome to Amazon S3.");
        assert_eq!(
            payload_sha256,
            "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072"
        );

        let canonical_uri = uri_encode("/test$file.text", false);
        assert_eq!(canonical_uri, "/test%24file.text");

        let signed = sign_request(
            "PUT",
            &canonical_uri,
            &[],
            &[
                ("Date", "Fri, 24 May 2013 00:00:00 GMT"),
                ("Host", "examplebucket.s3.amazonaws.com"),
                ("x-amz-content-sha256", &payload_sha256),
                ("x-amz-date", "20130524T000000Z"),
                ("x-amz-storage-class", "REDUCED_REDUNDANCY"),
            ],
            &payload_sha256,
            AWS_AKID,
            AWS_SECRET,
            "us-east-1",
            now,
        );

        let expected_canonical_request = "\
PUT\n\
/test%24file.text\n\
\n\
date:Fri, 24 May 2013 00:00:00 GMT\n\
host:examplebucket.s3.amazonaws.com\n\
x-amz-content-sha256:44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072\n\
x-amz-date:20130524T000000Z\n\
x-amz-storage-class:REDUCED_REDUNDANCY\n\
\n\
date;host;x-amz-content-sha256;x-amz-date;x-amz-storage-class\n\
44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072";

        assert_eq!(signed.canonical_request, expected_canonical_request);

        let expected_string_to_sign = "\
AWS4-HMAC-SHA256\n\
20130524T000000Z\n\
20130524/us-east-1/s3/aws4_request\n\
9e0e90d9c76de8fa5b200d8c849cd5b8dc7a3be3951ddb7f6a76b4158342019d";

        assert_eq!(signed.string_to_sign, expected_string_to_sign);
        assert_eq!(
            signed.signature,
            "98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"
        );
    }

    #[test]
    fn test_uri_encode_s3_rules() {
        assert_eq!(uri_encode("a b", false), "a%20b");
        assert_eq!(uri_encode("ä", false), "%C3%A4");
        assert_eq!(uri_encode("a+b", false), "a%2Bb");
        assert_eq!(uri_encode("a/b/c", false), "a/b/c");
        assert_eq!(uri_encode("a/b/c", true), "a%2Fb%2Fc");
        assert_eq!(uri_encode("~._-", false), "~._-");
        assert_eq!(uri_encode("*", false), "%2A");
        assert_eq!(uri_encode("a//b/./x", false), "a//b/./x");
    }

    #[test]
    fn test_session_token_included_in_signed_headers() {
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 8, 0, 0).unwrap();
        let payload_sha = sha256_hex(b"hello");
        let signed = sign_request(
            "PUT",
            "/bucket/key.jpg",
            &[],
            &[
                ("host", "s3.us-east-1.amazonaws.com"),
                ("x-amz-content-sha256", &payload_sha),
                ("x-amz-date", "20261117T080000Z"),
                ("x-amz-security-token", "session-tok-123"),
            ],
            &payload_sha,
            AWS_AKID,
            AWS_SECRET,
            "us-east-1",
            now,
        );
        assert!(signed.signed_headers.contains("x-amz-security-token"));
        assert!(
            signed
                .canonical_request
                .contains("x-amz-security-token:session-tok-123\n")
        );
    }

    #[test]
    fn test_canonical_query_string_sorting() {
        let q = canonical_query_string(&[("b", "2"), ("a", "1 2"), ("a", "0")]);
        assert_eq!(q, "a=0&a=1%202&b=2");
    }
}
