#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <openssl/sha.h>
#include <openssl/evp.h>

// Encryption parameters from the file
static const uint8_t salt[16] = {
    0x51, 0xe6, 0x8d, 0x53, 0xbd, 0x52, 0xf1, 0xf5,
    0xb4, 0x84, 0x22, 0xee, 0xfe, 0x38, 0x0d, 0x11
};
static const uint8_t encryptedVerifier[16] = {
    0x18, 0xa9, 0x72, 0x9f, 0x24, 0x86, 0xd6, 0xd6,
    0x94, 0xdd, 0xd6, 0xfb, 0x02, 0xdb, 0x11, 0xf8
};
static const uint8_t encryptedVerifierHash[20] = {
    0x2e, 0x98, 0x79, 0x31, 0xa7, 0x28, 0xf6, 0x46,
    0x4f, 0x96, 0x92, 0x91, 0x8c, 0x0c, 0x3a, 0xc3,
    0xbc, 0x57, 0x10, 0xe4
};
static const int keySize = 128; // bits
static const int ITER_COUNT = 20000;

static int verify_password(const char *password) {
    // Step 1: UTF-16LE encode password
    size_t pwlen = strlen(password);
    uint8_t *pw_utf16 = malloc((pwlen + 1) * 2);
    for (size_t i = 0; i < pwlen; i++) {
        pw_utf16[i * 2] = password[i];
        pw_utf16[i * 2 + 1] = 0x00;
    }
    pw_utf16[pwlen * 2] = 0x00;
    pw_utf16[pwlen * 2 + 1] = 0x00;
    size_t pw_utf16_len = (pwlen + 1) * 2;

    // Step 2: h0 = SHA-1(salt + password_utf16le)
    uint8_t h0[SHA_DIGEST_LENGTH];
    SHA256_CTX sha1; // using SHA256_CTX as generic, but we need SHA1
    // Actually use EVP for SHA1
    EVP_MD_CTX *ctx = EVP_MD_CTX_new();
    EVP_DigestInit_ex(ctx, EVP_sha1(), NULL);
    EVP_DigestUpdate(ctx, salt, 16);
    EVP_DigestUpdate(ctx, pw_utf16, pw_utf16_len);
    unsigned int h0_len;
    EVP_DigestFinal_ex(ctx, h0, &h0_len);

    // Step 3: 20000 iterations of SHA-1(int32le(i) + h0)
    uint8_t input[4 + SHA_DIGEST_LENGTH];
    memcpy(input, h0, SHA_DIGEST_LENGTH);
    for (int i = 0; i < ITER_COUNT; i++) {
        input[0] = (uint8_t)(i & 0xFF);
        input[1] = (uint8_t)((i >> 8) & 0xFF);
        input[2] = (uint8_t)((i >> 16) & 0xFF);
        input[3] = (uint8_t)((i >> 24) & 0xFF);
        EVP_DigestInit_ex(ctx, EVP_sha1(), NULL);
        EVP_DigestUpdate(ctx, input, 4 + SHA_DIGEST_LENGTH);
        EVP_DigestFinal_ex(ctx, h0, &h0_len);
    }

    // Step 4: hfinal = SHA-1(h0 + int32le(0))
    input[0] = input[1] = input[2] = input[3] = 0x00;
    memcpy(input + 4, h0, SHA_DIGEST_LENGTH);
    EVP_DigestInit_ex(ctx, EVP_sha1(), NULL);
    EVP_DigestUpdate(ctx, input, 4 + SHA_DIGEST_LENGTH);
    uint8_t hfinal[SHA_DIGEST_LENGTH];
    EVP_DigestFinal_ex(ctx, hfinal, &h0_len);

    // Step 5: Key derivation
    uint8_t buf1[64], buf2[64];
    for (int i = 0; i < 20; i++) {
        buf1[i] = hfinal[i] ^ 0x36;
        buf2[i] = hfinal[i] ^ 0x5c;
    }
    for (int i = 20; i < 64; i++) {
        buf1[i] = 0x36;
        buf2[i] = 0x5c;
    }

    uint8_t x1[SHA_DIGEST_LENGTH], x2[SHA_DIGEST_LENGTH];
    EVP_DigestInit_ex(ctx, EVP_sha1(), NULL);
    EVP_DigestUpdate(ctx, buf1, 64);
    EVP_DigestFinal_ex(ctx, x1, &h0_len);

    EVP_DigestInit_ex(ctx, EVP_sha1(), NULL);
    EVP_DigestUpdate(ctx, buf2, 64);
    EVP_DigestFinal_ex(ctx, x2, &h0_len);

    EVP_MD_CTX_free(ctx);
    free(pw_utf16);

    // Step 6: keyDerived = (x1 + x2)[:16]
    uint8_t keyDerived[16];
    memcpy(keyDerived, x1, 8);
    memcpy(keyDerived + 8, x2, 8);

    // Step 7: AES-ECB decrypt encryptedVerifier
    EVP_CIPHER_CTX *cctx = EVP_CIPHER_CTX_new();
    EVP_DecryptInit_ex(cctx, EVP_aes_128_ecb(), NULL, keyDerived, NULL);
    EVP_CIPHER_CTX_set_padding(cctx, 0);
    uint8_t verifier[16];
    int verifier_len;
    EVP_DecryptUpdate(cctx, verifier, &verifier_len, encryptedVerifier, 16);
    EVP_DecryptFinal_ex(cctx, verifier + verifier_len, &verifier_len);

    // Step 8: SHA-1(verifier) == encryptedVerifierHash?
    uint8_t computed[SHA_DIGEST_LENGTH];
    ctx = EVP_MD_CTX_new();
    EVP_DigestInit_ex(ctx, EVP_sha1(), NULL);
    EVP_DigestUpdate(ctx, verifier, 16);
    unsigned int computed_len;
    EVP_DigestFinal_ex(ctx, computed, &computed_len);
    EVP_MD_CTX_free(ctx);
    EVP_CIPHER_CTX_free(cctx);

    return memcmp(computed, encryptedVerifierHash, 20) == 0;
}

static void decrypt_file(const char *password) {
    // Just report success; Python will handle actual decryption
    printf("Password confirmed: %s\n", password);
}

int main(int argc, char **argv) {
    int start = 0, end = 1000000;
    if (argc > 1) start = atoi(argv[1]);
    if (argc > 2) end = atoi(argv[2]);

    printf("Cracking 6-digit PINs %d to %d...\n", start, end - 1);
    fflush(stdout);

    char buf[7];
    for (int i = start; i < end; i++) {
        snprintf(buf, 7, "%06d", i);
        if (verify_password(buf)) {
            printf("\n*** FOUND: %s ***\n", buf);
            return 0;
        }
        if ((i - start + 1) % 100000 == 0) {
            printf("  tried %d...\n", i - start + 1);
            fflush(stdout);
        }
    }

    printf("NOT FOUND in range %d-%d\n", start, end - 1);
    return 1;
}
