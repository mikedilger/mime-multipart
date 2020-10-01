# mime-multipart

[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE-MIT)
[![Apache-2.0 licensed](https://img.shields.io/badge/license-APACHE2-blue.svg)](./LICENSE-APACHE)

Rust library for MIME multipart parsing, construction, and streaming

This crate predates rust async support. It will remain pre-async to support
codebases which aren't intending to be rewritten under the async methodology.
That means we will remain on hyper 0.10.

Documentation is available at https://docs.rs/mime-multipart

## Features

* Parses from a stream, rather than in memory, so that memory is not hogged.
* Streams parts which are identified as files (via the part's Content-Disposition header,
  if any, or via a manual override) to files on disk.
* Uses buffered streams.
* Lets you build and stream out a multipart as a vector of parts (`Node`s), some of which
  could be files, others could be nested multipart parts.

If you are specifically dealing with `multipart/formdata`, you may be interested in
https://github.com/mikedilger/formdata which uses this crate and takes it a step
further.

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE)
    or http://www.apache.org/licenses/LICENSE-2.0)

 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
