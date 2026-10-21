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
fn sha1_vectors() {
    assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(
        hex(&sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
        "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
    );
    assert_eq!(
        hex(&sha1(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu")),
        "a49b2446a02c645bf419f995b67091253a04a259"
    );
    let mut h = Sha1::new();
    for _ in 0..1000 {
        h.update(&[b'a'; 1000]);
    }
    assert_eq!(hex(&h.finish()), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
}

#[test]
fn sha384_vectors() {
    assert_eq!(hex(&sha384(b"")), "38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b");
    assert_eq!(hex(&sha384(b"abc")), "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7");
    assert_eq!(
        hex(&sha384(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
        "3391fdddfc8dc7393707a65b1b4709397cf8b1d162af05abfe8f450de5f36bc6b0455a8520bc4e6f5fe95b1fe3c8452b"
    );
    assert_eq!(
        hex(&sha384(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu")),
        "09330c33f71147e83d192fc782cd1b4753111b173b3b05d22fa08086e3b0f712fcc7c71a557e2db966c3e9fa91746039"
    );
    let mut h = Sha512::sha384();
    for _ in 0..1000 {
        h.update(&[b'a'; 1000]);
    }
    assert_eq!(hex(&h.finish()), "9d0e1809716474cb086e834e310a4a1ced149e9c00f248527972cec5704c2a5b07b8b3dc38ecc4ebae97ddd87f3d8985");
}

#[test]
fn sha512_vectors() {
    assert_eq!(hex(&sha512(b"")), "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e");
    assert_eq!(hex(&sha512(b"abc")), "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f");
    assert_eq!(
        hex(&sha512(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
        "204a8fc6dda82f0a0ced7beb8e08a41657c16ef468b228a8279be331a703c33596fd15c13b1b07f9aa1d3bea57789ca031ad85c7a71dd70354ec631238ca3445"
    );
    assert_eq!(
        hex(&sha512(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu")),
        "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"
    );
    let mut h = Sha512::new();
    for _ in 0..1000 {
        h.update(&[b'a'; 1000]);
    }
    assert_eq!(hex(&h.finish()), "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973ebde0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b");
}

#[test]
fn digest_algorithms_by_web_crypto_name() {
    let data = b"Hello, World!";
    let lengths: Vec<(String, usize)> = ["SHA-1", "sha-256", "SHA-384", "SHA-512"]
        .iter()
        .map(|name| {
            let alg: DigestAlgorithm = name.parse().unwrap();
            (alg.name().to_string(), alg.digest(data).len())
        })
        .collect();
    assert_eq!(lengths, [("SHA-1".into(), 20), ("SHA-256".into(), 32), ("SHA-384".into(), 48), ("SHA-512".into(), 64)]);
    assert_eq!(hex(&DigestAlgorithm::Sha512.digest(data)), "374d794a95cdcfd8b35993185fef9ba368f160d8daf432d08ba9f1ed1e5abe6cc69291e0fa2fe0006a52570ef18c19def4e617c33ce52ef0a6e5fbe318cb0387");
    assert!("MD5".parse::<DigestAlgorithm>().unwrap_err().starts_with("NotSupportedError"));
    assert_eq!(DigestAlgorithm::default(), DigestAlgorithm::Sha256);
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
