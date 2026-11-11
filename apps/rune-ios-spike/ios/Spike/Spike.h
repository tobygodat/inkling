// The C ABI `apps/rune-ios-spike/src/lib.rs` exports. Main thread only. Points are UIKit points.
// Text offsets are UTF-16 units of the document UIKit sees (the text with any marked text in it).
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct SpikeHost SpikeHost;
typedef void (*SpikeWakeFn)(void *context);

SpikeHost *spike_new(void *layer, float width, float height, float scale, SpikeWakeFn wake, void *wake_context);
void spike_free(SpikeHost *host);
void spike_resize(SpikeHost *host, float width, float height, float scale);
void spike_set_insets(SpikeHost *host, float top, float right, float bottom, float left);
// Draws one frame. Returns whether it wants more; `wake_after` gets seconds to a timer, or -1.
bool spike_frame(SpikeHost *host, double *wake_after);
void spike_touch(SpikeHost *host, uint64_t id, uint32_t phase, float x, float y, double time);
bool spike_focused(SpikeHost *host);

// Tier 1: UIKeyInput.
void spike_insert_text(SpikeHost *host, const char *text);
void spike_delete_backward(SpikeHost *host);
bool spike_has_text(SpikeHost *host);

// Tier 2: UITextInput.
char *spike_text(SpikeHost *host);
void spike_string_free(char *s);
uint64_t spike_revision(SpikeHost *host);
void spike_selection(SpikeHost *host, size_t *start, size_t *end);
void spike_set_selection(SpikeHost *host, size_t start, size_t end);
bool spike_marked_range(SpikeHost *host, size_t *start, size_t *end);
void spike_replace(SpikeHost *host, size_t start, size_t end, const char *text);
void spike_set_marked(SpikeHost *host, const char *text, size_t sel_start, size_t sel_end);
void spike_unmark(SpikeHost *host);
bool spike_caret_rect(SpikeHost *host, float *out_xywh);
