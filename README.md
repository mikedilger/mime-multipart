# mime-multipart

[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE-MIT)
[![Apache-2.0 licensed](https://img.shields.io/badge/license-APACHE2-blue.svg)](./LICENSE-APACHE)

Rust library for MIME multipart parsing and construction

Documentation is available at https://mikedilger.github.io/mime-multipart

## Limitations

Currently we are not generating 'multipart/*', but this will be quite easy to do
once someone needs such functionality.  See issue #1.

Currently we require hyper::Headers passed in.  We could easily parse headers and
body ourselves, not requiring anything but the stream (and still leave the current
API for hyper users who already have the stream and headers separately).  See
issue #2.

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
