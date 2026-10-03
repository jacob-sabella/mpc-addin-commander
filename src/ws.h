// The server: HTTP (/, /info, /plugins, /skin/<id>/<path>) and the WebSocket at /ws.
#ifndef WS_H
#define WS_H
#include <stddef.h>

void *server_thread(void *arg);
// A text frame to every WebSocket client (a client whose send fails is dropped).
void ws_broadcast(const char *json, size_t n);
// Some client subscribed to this instance id.
int ws_subscribed(int id);
// The plugin at this .so path ships Plugin Skins/TUI.json.
int ws_skin_exists(const char *so_path);

#endif
