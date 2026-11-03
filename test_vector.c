#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <openssl/sha.h>
#include <openssl/evp.h>

static const int ITER_COUNT = 50000;

static void makekey(const char *password, const uint8_t *salt, int saltSize,
                    int keySize, uint8_t *keyDerived) {
    size_t pwlen = strlen(password);
    uint8_t *pw_utf16 = malloc((pwlen + 1) * 2);
    for (size_t i = 0; i < pwlen; i++) {
        pw_utf16[i * 2] = password[i];
        pw_utf16[i * 2 + 1] = 0x00;
    }
    pw_utf16[pwlen * 2] = 0x00;
    pw_utf16[pwlen * 2 + 1] = 0x00;
    size_t pw_utf16_len = (pwlen + 1) * 2;

    EVP_MD_CTX *ctx = EVP_MD_CTX_new();
    EVP_DigestInit_ex(ctx, EVP_sha1(), NULL);
    EVP_DigestUpdate(ctx, salt, saltSize);
    EVP_DigestUpdate(ctx, pw_utf16, pw_utf16_len);
    unsigned int h0_len;
    uint8_t h0[SHA_DIGEST_LENGTH];
    EVP_DigestFinal_ex(ctx, h0, &h0_len);

    uint8_t input[4 + SHA_DIGEST_LENGTH];
    memcpy(input + 4, h0, SHA_DIGEST_LENGTH);
    for (int i = 0; i < ITER_COUNT; i++) {
        input[0] = (uint8_t)(i & 0xFF);
        input[1] = (uint8_t)((i >> 8) & 0xFF);
        input[2] = (uint8_t)((i >> 16) & 0xFF);
        input[3] = (uint8_t)((i >> 24) & 0xFF);
        EVP_DigestInit_ex(ctx, EVP_sha1(), NULL);
        EVP_DigestUpdate(ctx, input, 4);
        EVP_DigestUpdate(ctx, h0, SHA_DIGEST_LENGTH);
        EVP_DigestFinal_ex(ctx, h0, &h0_len);
    }

    uint8_t blockbytes[4] = {0, 0, 0, 0};
    EVP_DigestInit_ex(ctx, EVP_sha1(), NULL);
    EVP_DigestUpdate(ctx, h0, SHA_DIGEST_LENGTH);
    EVP_DigestUpdate(ctx, blockbytes, 4);
    uint8_t hfinal[SHA_DIGEST_LENGTH];
    EVP_DigestFinal_ex(ctx, hfinal, &h0_len);

    int cbHash = SHA_DIGEST_LENGTH;
    uint8_t buf1[64], buf2[64];
    memset(buf1, 0x36, 64);
    memset(buf2, 0x5c, 64);
    for (int i = 0; i < cbHash; i++) {
        buf1[i] = hfinal[i] ^ 0x36;
        buf2[i] = hfinal[i] ^ 0x5c;
    }

    uint8_t x1[SHA_DIGEST_LENGTH], x2[SHA_DIGEST_LENGTH];
    EVP_DigestInit_ex(ctx, EVP_sha1(), NULL);
    EVP_DigestUpdate(ctx, buf1, 64);
    EVP_DigestFinal_ex(ctx, x1, &h0_len);

    EVP_DigestInit_ex(ctx, EVP_sha1(), NULL);
    EVP_DigestUpdate(ctx, buf2, 64);
    EVP_DigestFinal_ex(ctx, x2, &h0_len);

    // x3 = x1 + x2, keyDerived = x3[:keySize/8]
    memcpy(keyDerived, x1, keySize / 8);

    EVP_MD_CTX_free(ctx);
    free(pw_utf16);
}

int main() {
    const char *password = "Password1234_";
    uint8_t salt[] = {0xe8, 0x82, 0x66, 0x49, 0x0c, 0x5b, 0xd1, 0xee,
                      0xbd, 0x2b, 0x43, 0x94, 0xe3, 0xf8, 0x30, 0xef};
    uint8_t expected[] = {0x40, 0xb1, 0x3a, 0x71, 0xf9, 0x0b, 0x96, 0x6e,
                          0x37, 0x54, 0x08, 0xf2, 0xd1, 0x81, 0xa1, 0xaa};
    uint8_t derived[16];

    makekey(password, salt, 16, 128, derived);

    printf("Derived: ");
    for (int i = 0; i < 16; i++) printf("%02x", derived[i]);
    printf("\nExpected: ");
    for (int i = 0; i < 16; i++) printf("%02x", expected[i]);
    printf("\nMatch: %s\n", memcmp(derived, expected, 16) == 0 ? "YES" : "NO");

    return 0;
}
