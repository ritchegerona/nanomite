#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <openssl/sha.h>
#include <openssl/evp.h>
#include <openssl/aes.h>

#define ITER_COUNT 50000

static unsigned char salt[16];
static unsigned char encrypted_verifier[16];
static unsigned char encrypted_verifier_hash[32];
static int hash_verifier_hash_size = 32;

static void hex_to_bytes(const char *hex, unsigned char *out, int len) {
    for (int i = 0; i < len; i++) {
        unsigned int v;
        sscanf(hex + i*2, "%2x", &v);
        out[i] = (unsigned char)v;
    }
}

/* XOR two byte arrays */
static void xor_bytes(const unsigned char *a, const unsigned char *b, unsigned char *out, int len) {
    for (int i = 0; i < len; i++) {
        out[i] = a[i] ^ b[i];
    }
}

/* Derive intermediate key from password using ECMA-376 Standard algorithm */
static void makekey_from_password(const char *password, unsigned char *key_out) {
    /* Step 1: Encode password as UTF-16LE */
    int pwd_len = strlen(password);
    unsigned char *pwd_utf16 = malloc((pwd_len + 1) * 2);
    for (int i = 0; i < pwd_len; i++) {
        pwd_utf16[i*2] = (unsigned char)password[i];
        pwd_utf16[i*2+1] = 0;
    }
    pwd_utf16[pwd_len*2] = 0;
    pwd_utf16[pwd_len*2+1] = 0;
    int pwd_utf16_len = (pwd_len + 1) * 2;

    /* Step 2: h = SHA1(salt + password_utf16le) */
    unsigned char h[SHA_DIGEST_LENGTH];
    SHA_CTX ctx;
    SHA1_Init(&ctx);
    SHA1_Update(&ctx, salt, 16);
    SHA1_Update(&ctx, pwd_utf16, pwd_utf16_len);
    SHA1_Final(h, &ctx);

    /* Step 3: Iterate ITER_COUNT times: h = SHA1(i_bytes + h) */
    for (int i = 0; i < ITER_COUNT; i++) {
        unsigned char ibytes[4];
        ibytes[0] = (unsigned char)(i & 0xFF);
        ibytes[1] = (unsigned char)((i >> 8) & 0xFF);
        ibytes[2] = (unsigned char)((i >> 16) & 0xFF);
        ibytes[3] = (unsigned char)((i >> 24) & 0xFF);
        SHA1_Init(&ctx);
        SHA1_Update(&ctx, ibytes, 4);
        SHA1_Update(&ctx, h, SHA_DIGEST_LENGTH);
        SHA1_Final(h, &ctx);
    }

    /* Step 4: HMAC-SHA1 */
    /* block = 0 */
    unsigned char blockbytes[4] = {0, 0, 0, 0};
    unsigned char hfinal[SHA_DIGEST_LENGTH];
    SHA1_Init(&ctx);
    SHA1_Update(&ctx, h, SHA_DIGEST_LENGTH);
    SHA1_Update(&ctx, blockbytes, 4);
    SHA1_Final(hfinal, &ctx);

    /* buf1 = 0x36 * 64, XOR with hfinal (first 20 bytes) */
    unsigned char buf1[64], buf2[64];
    memset(buf1, 0x36, 64);
    memset(buf2, 0x5c, 64);

    unsigned char xor1[20], xor2[20];
    xor_bytes(hfinal, buf1, xor1, 20);
    xor_bytes(hfinal, buf2, xor2, 20);

    /* x1 = SHA1(xor1 || buf1[20:]) */
    unsigned char x1[SHA_DIGEST_LENGTH], x2[SHA_DIGEST_LENGTH];
    SHA1_Init(&ctx);
    SHA1_Update(&ctx, xor1, 20);
    SHA1_Update(&ctx, buf1 + 20, 44);
    SHA1_Final(x1, &ctx);

    /* x2 = SHA1(xor2 || buf2[20:]) */
    SHA1_Init(&ctx);
    SHA1_Update(&ctx, xor2, 20);
    SHA1_Update(&ctx, buf2 + 20, 44);
    SHA1_Final(x2, &ctx);

    /* keyDerived = first 16 bytes of (x1 || x2) */
    memcpy(key_out, x1, 16);

    free(pwd_utf16);
}

/* Verify: AES-ECB decrypt verifier with key, check SHA1(verifier) matches */
static int verify_key(const unsigned char *key) {
    AES_KEY aes_key;
    AES_set_decrypt_key(key, 128, &aes_key);

    unsigned char verifier[16];
    AES_ecb_encrypt(encrypted_verifier, verifier, &aes_key, AES_DECRYPT);

    /* SHA1(verifier) should equal first hash_verifier_hash_size bytes of decrypted verifierHash */
    unsigned char verifier_hash[SHA_DIGEST_LENGTH];
    SHA1(verifier, 16, verifier_hash);

    unsigned char decrypted_hash[32];
    AES_ecb_encrypt(encrypted_verifier_hash, decrypted_hash, &aes_key, AES_DECRYPT);

    return memcmp(verifier_hash, decrypted_hash, hash_verifier_hash_size) == 0;
}

int try_password(const char *password) {
    unsigned char key[16];
    makekey_from_password(password, key);
    return verify_key(key);
}

int main(int argc, char *argv[]) {
    hex_to_bytes("51e68d53bd52f1f5b48422eefe380d11", salt, 16);
    hex_to_bytes("18a9729f2486d6d694ddd6fb02db11f8", encrypted_verifier, 16);
    hex_to_bytes("2e987931a728f6f464f9692918c0c3acbc5710e4", encrypted_verifier_hash, 32);
    hash_verifier_hash_size = 32;

    char password[256];
    int count = 0;
    while (fgets(password, sizeof(password), stdin)) {
        size_t len = strlen(password);
        while (len > 0 && (password[len-1] == '\n' || password[len-1] == '\r'))
            password[--len] = '\0';
        if (len == 0) continue;

        count++;
        if (try_password(password)) {
            printf("FOUND: %s\n", password);
            fflush(stdout);
            return 0;
        }

        if (count % 100 == 0) {
            fprintf(stderr, "tested %d...\r", count);
        }
    }

    fprintf(stderr, "\ntested %d total, no match\n", count);
    return 1;
}