/* Links the generated header against the static library and exercises the
 * full C ABI lifecycle. Run through tools/ffi-smoke/run.sh. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "orbit.h"

static int fail(const char *step, int32_t status) {
    OrbitBuffer message = {0};
    orbit_last_error_message(&message);
    fprintf(stderr, "%s failed with %d: %.*s\n", step, status, (int)message.len, (const char *)message.data);
    orbit_buffer_free(message);
    return 1;
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: %s <absolute-data-dir>\n", argv[0]);
        return 2;
    }
    if (orbit_abi_version() != ORBIT_ABI_VERSION) {
        fprintf(stderr, "ABI version mismatch\n");
        return 1;
    }

    OrbitBuffer secret = {0};
    int32_t status = orbit_identity_generate(&secret);
    if (status != ORBIT_OK) return fail("identity_generate", status);

    char config[4096];
    int config_len = snprintf(config, sizeof config, "{\"data_dir\":\"%s\"}", argv[1]);
    uint64_t engine = 0;
    status = orbit_engine_open((const uint8_t *)config, (size_t)config_len, secret.data, secret.len, &engine);
    orbit_buffer_free(secret);
    if (status != ORBIT_OK) return fail("engine_open", status);

    const char *command = "{\"type\":\"get_snapshot\"}";
    uint64_t request = 0;
    status = orbit_engine_submit(engine, (const uint8_t *)command, strlen(command), &request);
    if (status != ORBIT_OK) return fail("engine_submit", status);

    OrbitBuffer events = {0};
    status = orbit_engine_wait_events(engine, 5000, &events);
    if (status != ORBIT_OK) return fail("engine_wait_events", status);
    int has_result = events.len > 0 && strstr((const char *)events.data, "command_succeeded") != NULL;
    printf("%.*s\n", (int)events.len, (const char *)events.data);
    orbit_buffer_free(events);
    if (!has_result) {
        fprintf(stderr, "no command result in first batch\n");
        return 1;
    }

    status = orbit_engine_close(engine);
    if (status != ORBIT_OK) return fail("engine_close", status);
    status = orbit_engine_wait_events(engine, 0, &events);
    if (status != ORBIT_ERR_CLOSED) {
        fprintf(stderr, "wait after close returned %d\n", status);
        return 1;
    }
    puts("ffi smoke: ok");
    return 0;
}
