#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <openssl/evp.h>
#include <openssl/sha.h>
#include <openssl/rand.h>

/* Office 2007+ AES-128-CBC key derivation and decryption using OpenSSL */

static int spin_count = 20000;
static const char *salt_hex = "51e68d53bd52f1f5b48422eefe380d11";
static const char *encrypted_key_hex = "18a9729f2486d6d694ddd6fb02db11f8";
static const char *hash_verifier_hex = "2e987931a728f6f464f9692918c0c3acbc5710e4";

static unsigned char salt[16], encrypted_key[16], hash_verifier[16];

static void hex_to_bytes(const char *hex, unsigned char *out, int len) {
    for (int i = 0; i < len; i++) {
        sscanf(hex + i*2, "%2hhx", &out[i]);
    }
}

static void bytes_to_hex(const unsigned char *in, char *out, int len) {
    for (int i = 0; i < len; i++) {
        sprintf(out + i*2, "%02x", in[i]);
    }
    out[len*2] = '\0';
}

/* Derive key from password using SHA-1 with spin count iterations
   This matches msoffcrypto's makekey_from_password */
static void derive_key(const char *password, unsigned char *key_out, unsigned char *verifier_out) {
    /* Step 1: SHA1(salt + password) */
    unsigned char h[SHA_DIGEST_LENGTH];
    SHA_CTX ctx;
    SHA1_Init(&ctx);
    SHA1_Update(&ctx, salt, 16);
    SHA1_Update(&ctx, password, strlen(password));
    SHA1_Final(h, &ctx);

    /* Step 2: Iterate spin_count times */
    for (int i = 0; i < spin_count; i++) {
        SHA1_Init(&ctx);
        SHA1_Update(&ctx, h, SHA_DIGEST_LENGTH);
        SHA1_Final(h, &ctx);
    }

    /* Step 3: Key = first 16 bytes of h */
    memcpy(key_out, h, 16);

    /* Step 4: Verifier = SHA1(key + salt) */
    SHA1_Init(&ctx);
    SHA1_Update(&ctx, key_out, 16);
    SHA1_Update(&ctx, salt, 16);
    SHA1_Final(verifier_out, &ctx);
}

/* Try to decrypt encrypted_key with derived key and check verifier */
static int try_password(const char *password) {
    unsigned char key[16], verifier[20];
    derive_key(password, key, verifier);

    /* Compare first 16 bytes of verifier with hash_verifier */
    return memcmp(verifier, hash_verifier, 16) == 0;
}

int main(int argc, char *argv[]) {
    hex_to_bytes(salt_hex, salt, 16);
    hex_to_bytes(encrypted_key_hex, encrypted_key, 16);
    hex_to_bytes(hash_verifier_hex, hash_verifier, 16);

    if (argc < 2) {
        fprintf(stderr, "Usage: %s <password>\n", argv[0]);
        return 1;
    }

    if (try_password(argv[1])) {
        printf("FOUND: %s\n", argv[1]);
        return 0;
    } else {
        return 1;
    }
}