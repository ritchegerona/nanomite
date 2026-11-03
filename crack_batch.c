#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <openssl/sha.h>

static const char *salt_hex = "51e68d53bd52f1f5b48422eefe380d11";
static const char *hash_verifier_hex = "2e987931a728f6f464f9692918c0c3acbc5710e4";

static unsigned char salt[16], hash_verifier[16];
static int spin_count = 20000;

static void hex_to_bytes(const char *hex, unsigned char *out, int len) {
    for (int i = 0; i < len; i++) {
        unsigned int v;
        sscanf(hex + i*2, "%2x", &v);
        out[i] = (unsigned char)v;
    }
}

static void derive_key(const char *password, unsigned char *key_out, unsigned char *verifier_out) {
    unsigned char h[SHA_DIGEST_LENGTH];
    SHA_CTX ctx;
    SHA1_Init(&ctx);
    SHA1_Update(&ctx, salt, 16);
    SHA1_Update(&ctx, password, strlen(password));
    SHA1_Final(h, &ctx);

    for (int i = 0; i < spin_count; i++) {
        SHA1_Init(&ctx);
        SHA1_Update(&ctx, h, SHA_DIGEST_LENGTH);
        SHA1_Final(h, &ctx);
    }

    memcpy(key_out, h, 16);

    SHA1_Init(&ctx);
    SHA1_Update(&ctx, key_out, 16);
    SHA1_Update(&ctx, salt, 16);
    SHA1_Final(verifier_out, &ctx);
}

int main() {
    hex_to_bytes(salt_hex, salt, 16);
    hex_to_bytes(hash_verifier_hex, hash_verifier, 16);

    char password[256];
    int count = 0;
    while (fgets(password, sizeof(password), stdin)) {
        // strip newline
        size_t len = strlen(password);
        while (len > 0 && (password[len-1] == '\n' || password[len-1] == '\r'))
            password[--len] = '\0';
        if (len == 0) continue;

        unsigned char key[16], verifier[20];
        derive_key(password, key, verifier);

        count++;
        if (memcmp(verifier, hash_verifier, 16) == 0) {
            printf("FOUND: %s\n", password);
            fflush(stdout);
            return 0;
        }

        if (count % 1000 == 0) {
            fprintf(stderr, "tested %d...\n", count);
        }
    }

    fprintf(stderr, "tested %d total, no match\n", count);
    return 1;
}