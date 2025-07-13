<!--
SPDX-FileCopyrightText: 2025 LunNova
SPDX-License-Identifier: CC0-1.0
-->

# atproto-comments

Article: [atproto as a static site's comment section @ lunnova.dev](https://lunnova.dev/articles/atproto-static-site-comments/)

Basic usage:

```
<div id="comment-section"></div>
<script type="module" defer>
import Comments from '/atproto-comments.js';
new Comments(
    document.getElementById('comment-section'), // where to inject the comments
    "/comments.css", // comments specific CSS
    'https://public.api.bsky.app/', // AppView base URL for API call
    //'at://did:plc:j3hvz7sryv6ese4nuug2djn7/post/3ltikv7zewc2l' // URI of the root of the thread to load
).render();
</script>
```

Copying into your own site and modifying as needed encouraged.
