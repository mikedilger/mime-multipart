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

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

pub use error::Error;

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::borrow::Cow;
use std::ops::Drop;
use encoding::{all, Encoding, DecoderTrap};
use hyper::header::{ContentType, Headers, ContentDisposition, DispositionParam,
                    DispositionType, Charset};
use tempdir::TempDir;
use textnonce::TextNonce;
use mime::{Attr, Mime, TopLevel, Value};
use buf_read_ext::BufReadExt;

/// A multipart part which is not a file (stored in memory)
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    headers: Headers,
    body: Vec<u8>,
}
impl Part {
    /// Mime content-type specified in the header
    pub fn content_type(&self) -> Option<Mime> {
        let ct: Option<&ContentType> = self.headers.get();
        ct.map(|ref ct| ct.0.clone())
    }
}

/// An uploaded file that was received as part of `multipart/*` parsing.
/// Files are streamed to disk to conserve memory (files are potentially very
/// large)
#[derive(Clone, Debug, PartialEq)]
pub struct FilePart {
    /// The complete headers that were sent along with the file body.
    pub headers: Headers,
    /// The temporary file where the file body was saved.
    pub path: PathBuf,
    /// The size of the file.
    pub size: usize,
    // The temporary directory the upload was put into, saved for the Drop trait
    tempdir: PathBuf,
}
impl FilePart {
    pub fn new(headers: Headers) -> Result<FilePart, Error> {
        // Setup a file to capture the contents.
        let tempdir = try!(TempDir::new("mime_multipart")).into_path();
        let mut path = tempdir.clone();
        path.push(TextNonce::sized_urlsafe(32).unwrap().into_string());
        Ok(FilePart {
            path: path,
            size: 0,
            headers: headers,
            tempdir: tempdir,
        })
    }

    /// Filename that was specified when the file was uploaded.  Returns `Ok<None>` if there
    /// was no content-disposition header supplied.
    pub fn filename(&self) -> Result<Option<String>, Error> {
        let cd: Option<&ContentDisposition> = self.headers.get();
        match cd {
            Some(cd) => get_content_disposition_filename(cd),
            None => Ok(None),
        }
    }

    /// Mime content-type specified in the header
    pub fn content_type(&self) -> Option<Mime> {
        let ct: Option<&ContentType> = self.headers.get();
        ct.map(|ref ct| ct.0.clone())
    }
}
impl Drop for FilePart {
    fn drop(&mut self) {
        let _ = ::std::fs::remove_file(&self.path);
        let _ = ::std::fs::remove_dir(&self.tempdir);
    }
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

/// Parse a MIME `multipart/*` from a `Read`able stream into a `Vec` of `Node`s, streaming
/// files to disk and keeping the rest in memory.  Recursive `multipart/*` parts will are
/// parsed as well and returned within a `Node::Multipart` variant.
///
/// If `always_use_files` is true, all parts will be streamed to files.  If false, only parts
/// with a `ContentDisposition` header set to `Attachment` or otherwise containing a `Filename`
/// parameter will be streamed to files.
///
/// It is presumed that the headers are still in the stream.  If you have them separately,
/// use `parse_multipart_body()` instead.
pub fn parse_multipart<S: Read>(
    stream: &mut S,
    always_use_files: bool)
    -> Result<Vec<Node>, Error>
{
    let mut reader = BufReader::with_capacity(4096, stream);
    let mut nodes: Vec<Node> = Vec::new();

    let mut buf: Vec<u8> = Vec::new();

    let (_, found) = try!(reader.stream_until_token(b"\r\n\r\n", &mut buf));
    if ! found { return Err(Error::Eof); }

    // Keep the CRLFCRLF as httparse will expect it
    buf.extend(b"\r\n\r\n".iter().cloned());

    // Parse the headers
    let mut header_memory = [httparse::EMPTY_HEADER; 16];
    let headers = try!(match httparse::parse_headers(&buf, &mut header_memory) {
        Ok(httparse::Status::Complete((_, raw_headers))) => {
            Headers::from_raw(raw_headers).map_err(|e| From::from(e))
        },
        Ok(httparse::Status::Partial) => Err(Error::PartialHeaders),
        Err(err) => Err(From::from(err)),
    });

    try!(inner(&mut reader, &headers, &mut nodes, always_use_files));
    Ok(nodes)
}

/// Parse a MIME `multipart/*` from a `Read`able stream into a `Vec` of `Node`s, streaming
/// files to disk and keeping the rest in memory.  Recursive `multipart/*` parts will are
/// parsed as well and returned within a `Node::Multipart` variant.
///
/// If `always_use_files` is true, all parts will be streamed to files.  If false, only parts
/// with a `ContentDisposition` header set to `Attachment` or otherwise containing a `Filename`
/// parameter will be streamed to files.
///
/// It is presumed that you have the `Headers` already and the stream starts at the body.
/// If the headers are still in the stream, use `parse_multipart()` instead.
pub fn parse_multipart_body<S: Read>(
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

#[inline]
fn get_content_disposition_filename(cd: &ContentDisposition) -> Result<Option<String>, Error> {
    if let Some(&DispositionParam::Filename(ref charset, _, ref bytes)) =
        cd.parameters.iter().find(|&x| match *x {
            DispositionParam::Filename(_,_,_) => true,
            _ => false,
        })
    {
        match charset_decode(charset, bytes) {
            Ok(filename) => Ok(Some(filename)),
            Err(e) => Err(Error::Decoding(e)),
        }
    } else {
        Ok(None)
    }
}

// This decodes bytes encoded according to a hyper::header::Charset encoding, using the
// rust-encoding crate.  Only supports encodings defined in both crates.
fn charset_decode(charset: &Charset, bytes: &[u8]) -> Result<String, Cow<'static, str>> {
    Ok(match *charset {
        Charset::Us_Ascii => try!(all::ASCII.decode(bytes, DecoderTrap::Strict)),
        Charset::Iso_8859_1 => try!(all::ISO_8859_1.decode(bytes, DecoderTrap::Strict)),
        Charset::Iso_8859_2 => try!(all::ISO_8859_2.decode(bytes, DecoderTrap::Strict)),
        Charset::Iso_8859_3 => try!(all::ISO_8859_3.decode(bytes, DecoderTrap::Strict)),
        Charset::Iso_8859_4 => try!(all::ISO_8859_4.decode(bytes, DecoderTrap::Strict)),
        Charset::Iso_8859_5 => try!(all::ISO_8859_5.decode(bytes, DecoderTrap::Strict)),
        Charset::Iso_8859_6 => try!(all::ISO_8859_6.decode(bytes, DecoderTrap::Strict)),
        Charset::Iso_8859_7 => try!(all::ISO_8859_7.decode(bytes, DecoderTrap::Strict)),
        Charset::Iso_8859_8 => try!(all::ISO_8859_8.decode(bytes, DecoderTrap::Strict)),
        Charset::Iso_8859_9 => return Err("ISO_8859_9 is not supported".into()),
        Charset::Iso_8859_10 => try!(all::ISO_8859_10.decode(bytes, DecoderTrap::Strict)),
        Charset::Shift_Jis => return Err("Shift_Jis is not supported".into()),
        Charset::Euc_Jp => try!(all::EUC_JP.decode(bytes, DecoderTrap::Strict)),
        Charset::Iso_2022_Kr => return Err("Iso_2022_Kr is not supported".into()),
        Charset::Euc_Kr => return Err("Euc_Kr is not supported".into()),
        Charset::Iso_2022_Jp => try!(all::ISO_2022_JP.decode(bytes, DecoderTrap::Strict)),
        Charset::Iso_2022_Jp_2 => return Err("Iso_2022_Jp_2 is not supported".into()),
        Charset::Iso_8859_6_E => return Err("Iso_8859_6_E is not supported".into()),
        Charset::Iso_8859_6_I => return Err("Iso_8859_6_I is not supported".into()),
        Charset::Iso_8859_8_E => return Err("Iso_8859_8_E is not supported".into()),
        Charset::Iso_8859_8_I => return Err("Iso_8859_8_I is not supported".into()),
        Charset::Gb2312 => return Err("Gb2312 is not supported".into()),
        Charset::Big5 => try!(all::BIG5_2003.decode(bytes, DecoderTrap::Strict)),
        Charset::Koi8_R => try!(all::KOI8_R.decode(bytes, DecoderTrap::Strict)),
        Charset::Ext(ref s) => match &**s {
            "UTF-8" => try!(all::UTF_8.decode(bytes, DecoderTrap::Strict)),
            _ => return Err("Encoding is not supported".into()),
        },
    })
}
