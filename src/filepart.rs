// Copyright 2016 mime-multipart Developers
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use std::path::PathBuf;
use std::borrow::Cow;
use std::ops::Drop;
use encoding::{all, Encoding, DecoderTrap};
use mime::Mime;
use tempdir::TempDir;
use error::Error;
use textnonce::TextNonce;
use hyper::header::{ContentType, Headers, ContentDisposition,
                    DispositionParam, Charset};

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
