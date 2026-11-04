---
title: Vision input
tags: [concept, feature, images, openai]
sourceRefs: [src/app.rs, src/agent/provider.rs, src/agent/openai.rs, src/commands.rs]
lastReviewed: 2026-10-30
---

# Vision input

`/image <path>` (alias `/img`, alt+v) attaches images to the next message;
verified 2026-10-30 (518/518 lib tests).

## Pipeline

1. `build_image_attachment(arg, cwd)` validates: extension allow-list
   (png/jpg/jpeg/gif/webp → MIME), size cap 10 MB (checked before and after read),
   then encodes `data:{mime};base64,{payload}` with the RFC 4648 `base64_encode`
   helper.
2. `attach_image` queues a `PendingImage { name, data_url, bytes }` on the app
   (max 4; `Control::ImageAttached` via `spawn_blocking`; headless/tests compute
   inline). `/new` clears the queue.
3. `start_turn` does `std::mem::take(&mut self.pending_images)`, prepends
   `[image: name · size]` badge lines to the entry text, and builds
   `ChatMessage::user_with_images(prompt, data_urls)`.

## Wire format

`OpenAiProvider::message_to_wire` emits, for user messages with images, a parts
array: `{type:"text", text}` only when the prompt is non-empty, then
`{type:"image_url", image_url:{url}}` per image. All other roles (and images-less
messages) remain plain strings.

## Caveats

- Only the wire format is tested offline — verify the configured provider actually
  accepts OpenAI `image_url` parts (vision-capable model) before relying on it.
- Size labels use `{:.1}` MB formatting: 3.25 MB renders as "3.2 MB".
