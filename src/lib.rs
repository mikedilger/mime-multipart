// Copyright 2016 mime-multipart Developers
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

extern crate httparse;
extern crate hyper;
#[macro_use]
extern crate mime;
extern crate tempdir;
extern crate textnonce;
#[macro_use]
extern crate log;
extern crate encoding;
extern crate buf_read_ext;

pub mod error;
pub mod filepart;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

pub use error::Error;
pub use filepart::FilePart;

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use hyper::header::{ContentType, Headers, ContentDisposition, DispositionParam,
                    DispositionType};
use mime::{Attr, Mime, TopLevel, Value};
use buf_read_ext::BufReadExt;

/// A multipart part which is not a file (stored in memory)
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    headers: Headers,
    body: Vec<u8>,
}

/// A multipart part which could be either a file, in memory, or another multipart
/// container containing nested parts.
#[derive(Clone, Debug)]
pub enum Node {
    /// A part in memory
    Part(Part),
    /// A part streamed to a file
    File(FilePart),
    /// A container of nested multipart parts
    Multipart((Headers, Vec<Node>)),
}

/// Parse a MIME multipart/* into a `Vec` of `Node`s.  You must pass in a `Read`able
/// stream for reading the body, as well as the `Headers` separately.  If `always_use_files`
/// is true, all parts will be streamed to files.  If false, only parts with a `Filename`
/// `ContentDisposition` header will be streamed to files.  Recursive `multipart/*` parts
/// will are parsed as well and returned within a `Node::Multipart` variant.
pub fn parse_multipart<S: Read>(
    stream: &mut S,
    headers: &Headers,
    always_use_files: bool)
    -> Result<Vec<Node>, Error>
{
    let mut reader = BufReader::with_capacity(4096, stream);
    let mut nodes: Vec<Node> = Vec::new();
    try!(inner(&mut reader, headers, &mut nodes, always_use_files));
    Ok(nodes)
}

fn inner<R: BufRead>(
    reader: &mut R,
    headers: &Headers,
    nodes: &mut Vec<Node>,
    always_use_files: bool)
    -> Result<(), Error>
{
    let mut buf: Vec<u8> = Vec::new();

    let boundary = try!(get_multipart_boundary(headers));
    let crlf_boundary = prepend_crlf(&boundary);

    // Read past the initial boundary
    let (_, found) = try!(reader.stream_until_token(&boundary, &mut buf));
    if ! found { return Err(Error::Eof); }

    loop {
        // If the next two lookahead characters are '--', parsing is finished.
        {
            let peeker = try!(reader.fill_buf());
            if peeker.len() >= 2 && &peeker[..2] == b"--" {
                return Ok(());
            }
        }

        // Read the CRLF after the boundary
        let (_, found) = try!(reader.stream_until_token(b"\r\n", &mut buf));
        if ! found { return Err(Error::Eof); }

        // Read the headers (which end in CRLFCRLF)
        buf.truncate(0); // start fresh
        let (_, found) = try!(reader.stream_until_token(b"\r\n\r\n", &mut buf));
        if ! found { return Err(Error::Eof); }

        // Keep the CRLFCRLF as httparse will expect it
        buf.extend(b"\r\n\r\n".iter().cloned());

        // Parse the headers
        let mut header_memory = [httparse::EMPTY_HEADER; 4];
        let part_headers = try!(match httparse::parse_headers(&buf, &mut header_memory) {
            Ok(httparse::Status::Complete((_, raw_headers))) => {
                Headers::from_raw(raw_headers).map_err(|e| From::from(e))
            },
            Ok(httparse::Status::Partial) => Err(Error::PartialHeaders),
            Err(err) => Err(From::from(err)),
        });

        // Check for a nested multipart
        let nested = {
            let ct: Option<&ContentType> = part_headers.get();
            if let Some(ct) = ct {
                let &ContentType(Mime(ref top_level, _, _)) = ct;
                *top_level == TopLevel::Multipart
            } else {
                false
            }
        };
        if nested {
            // Recurse:
            let mut inner_nodes: Vec<Node> = Vec::new();
            try!(inner(reader, &part_headers, &mut inner_nodes, always_use_files));
            nodes.push(Node::Multipart((part_headers, inner_nodes)));
            continue;
        }

        let is_file = always_use_files || {
            let cd: Option<&ContentDisposition> = part_headers.get();
            if cd.is_some() {
                if cd.unwrap().disposition == DispositionType::Attachment {
                    true
                } else {
                    cd.unwrap().parameters.iter().any(|x| match x {
                        &DispositionParam::Filename(_,_,_) => true,
                        _ => false
                    })
                }
            } else {
                false
            }
        };
        if is_file {
            // Setup a file to capture the contents.
            let mut filepart = try!(FilePart::new(part_headers));
            let mut file = try!(File::create(filepart.path.clone()));

            // Stream out the file.
            let (read, found) = try!(reader.stream_until_token(&crlf_boundary, &mut file));
            if ! found { return Err(Error::Eof); }
            filepart.size = read;

            // TODO: Handle Content-Transfer-Encoding.  RFC 7578 section 4.7 deprecated
            // this, and the authors state "Currently, no deployed implementations that
            // send such bodies have been discovered", so this is very low priority.

            nodes.push(Node::File(filepart));
        } else {
            buf.truncate(0); // start fresh
            let (_, found) = try!(reader.stream_until_token(&crlf_boundary, &mut buf));
            if ! found { return Err(Error::Eof); }

            nodes.push(Node::Part(Part {
                headers: part_headers,
                body: buf.clone(),
            }));
        }
    }
}

/// Get the `multipart/*` boundary string from `hyper::Headers`
pub fn get_multipart_boundary(headers: &Headers) -> Result<Vec<u8>, Error> {
    // Verify that the request is 'Content-Type: multipart/*'.
    let ct: &ContentType = match headers.get() {
        Some(ct) => ct,
        None => return Err(Error::NoRequestContentType),
    };
    let ContentType(ref mime) = *ct;
    let Mime(ref top_level, _, ref params) = *mime;

    if *top_level != TopLevel::Multipart {
        return Err(Error::NotMultipart);
    }

    for &(ref attr, ref val) in params.iter() {
        if let (&Attr::Boundary, &Value::Ext(ref val)) = (attr, val) {
            let mut boundary = Vec::with_capacity(2 + val.len());
            boundary.extend(b"--".iter().cloned());
            boundary.extend(val.as_bytes());
            return Ok(boundary);
        }
    }
    Err(Error::BoundaryNotSpecified)
}

fn prepend_crlf(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(2 + input.len());
    output.extend(b"\r\n".iter().cloned());
    output.extend(input.clone());
    output
}
