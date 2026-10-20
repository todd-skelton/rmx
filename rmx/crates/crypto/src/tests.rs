use super::*;

#[test]
fn sha256_vectors() {
    assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(
        hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    let mut h = Sha256::new();
    for _ in 0..1000 {
        h.update(&[b'a'; 1000]);
    }
    assert_eq!(hex(&h.finish()), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
}

#[test]
fn hmac_vectors() {
    // RFC 4231 test cases 1, 2 and 6.
    assert_eq!(
        hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    assert_eq!(
        hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert_eq!(
        hex(&hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First")),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
}

#[test]
fn pbkdf2_vectors() {
    assert_eq!(
        hex(&pbkdf2_sha256(b"password", b"salt", 1, 32)),
        "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
    );
    assert_eq!(
        hex(&pbkdf2_sha256(b"password", b"salt", 4096, 32)),
        "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a"
    );
    assert_eq!(
        hex(&pbkdf2_sha256(b"passwordPASSWORDpassword", b"saltSALTsaltSALTsaltSALTsaltSALTsalt", 4096, 40)),
        "348c89dbcbd32b2f32d814b8116e84cf2b17347ebc1800181c4e2a1fb8dd53e1c635518c7dac47e9"
    );
}

#[test]
fn base64_round_trips() {
    assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    assert_eq!(base64_encode(b"fooba"), "Zm9vYmE");
    assert_eq!(base64_encode(b"f"), "Zg");
    assert_eq!(base64_decode("Zm9vYmE=").unwrap(), b"fooba");
    assert_eq!(base64url_encode(&[0xfb, 0xff]), "-_8");
    assert_eq!(base64url_decode("-_8").unwrap(), [0xfb, 0xff]);
    assert!(base64_decode("Zm9v!").is_none());
    for n in 0..40u8 {
        let data: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37)).collect();
        assert_eq!(base64_decode(&base64_encode(&data)).unwrap(), data);
    }
}

#[test]
fn randomness_and_comparison() {
    let a = random_uuid();
    assert_eq!(a.len(), 36);
    assert_eq!(&a[14..15], "4");
    assert_ne!(a, random_uuid());
    assert_eq!(random_hex(32).len(), 64);
    assert!(constant_time_eq(b"abc", b"abc"));
    assert!(!constant_time_eq(b"abc", b"abd"));
    assert!(!constant_time_eq(b"abc", b"abcd"));
}

#[test]
fn scrypt_vectors() {
    // RFC 7914 section 12, test vectors 1 and 2.
    let v1 = scrypt(b"", b"", ScryptParams { n: 16, r: 1, p: 1 }, 64).unwrap();
    assert_eq!(hex(&v1), "77d6576238657b203b19ca42c18a0497f16b4844e3074ae8dfdffa3fede21442fcd0069ded0948f8326a753a0fc81f17e8d3e0fb2e0d3628cf35e20c38d18906");
    let v2 = scrypt(b"password", b"NaCl", ScryptParams { n: 1024, r: 8, p: 16 }, 64).unwrap();
    assert_eq!(hex(&v2), "fdbabe1c9d3472007856e7190d01e9fe7c6ad7cbc8237830e77376634b3731622eaf30d92e22a3886ff109279d9830dac727afb94a83ee6d8360cbdfa2cc0640");
    // Node's crypto.scryptSync('password123', salt, 64) with its defaults (N=16384, r=8, p=1).
    let node = scrypt(b"password123", b"0123456789abcdef0123456789abcdef", ScryptParams::default(), 64).unwrap();
    assert_eq!(hex(&node), "7bfc2ad8e90d7252b24c322d25a3ef314bb6f3b0ad29738a3140d921aa2606024e5f3f7eef2b45dfb16dd5428705bb539af7384b2504308175cc714be9c428bf");
    assert_eq!(scrypt(b"x", b"y", ScryptParams { n: 3, r: 1, p: 1 }, 16), None, "n must be a power of two");
    assert_eq!(scrypt(b"x", b"y", ScryptParams { n: 1 << 20, r: 8, p: 1 }, 16), None, "beyond Node's 32 MiB maxmem");
}
