# Dynamic sites in pure serverless Rust
This repo is an example of how to build your website entirely in Rust, no
servers required. It uses Cloudflare Workers to handle requests. Instead of a
database, it uses Cloudflare Workers KV to read and write user data. This means
your requests can be rendered entirely on the edge, without having to go a
central database somewhere. So the application should be very low latency.

Another nice advantage of using Workers KV is that the data schema lives
entirely in Rust. You don't need to write a matching SQL table or type, which
means you don't have to worry about keeping the Rust schema and SQL schema in
sync.

Because all the logic is processed in Rust (not in JS) there's no need to store
data in JSON. Instead, we use MessagePack, which is like JSON but binary, and
therefore much faster to process.

Wrangler is an awesome tool for managing your Cloudflare workers. You can use
`wrangler dev` to test your worker using a real Cloudflare edge colo, and then
use `wrangler publish` to push your worker onto the internet, either at a
subdomain that you own, or at a free subdomain of workers.dev.