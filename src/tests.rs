// Copyright 2016 mime-multipart Developers
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use super::*;

use std::net::SocketAddr;

use hyper::buffer::BufReader;
use hyper::net::NetworkStream;
use hyper::server::Request as HyperRequest;

use mock::MockStream;

use hyper::header::{ContentDisposition, DispositionParam};

#[test]
fn parser() {
    let input = b"POST / HTTP/1.1\r\n\
                  Host: example.domain\r\n\
                  Content-Type: multipart/mixed; boundary=\"abcdefg\"\r\n\
                  Content-Length: 1000\r\n\
                  \r\n\
                  --abcdefg\r\n\
                  Content-Type: application/json\r\n\
                  \r\n\
                  {\r\n\
                    \"id\": 15\r\n\
                  }\r\n\
                  --abcdefg\r\n\
                  Content-Disposition: Attachment; filename=\"image.gif\"\r\n\
                  Content-Type: image/gif\r\n\
                  \r\n\
                  This is a file\r\n\
                  with two lines\r\n\
                  --abcdefg\r\n\
                  Content-Disposition: Attachment; filename=\"file.txt\"\r\n\
                  \r\n\
                  This is a file\r\n\
                  --abcdefg--";

    let mut mock = MockStream::with_input(input);

    let mock: &mut NetworkStream = &mut mock;
    let mut stream = BufReader::new(mock);
    let sock: SocketAddr = "127.0.0.1:80".parse().unwrap();
    let req = HyperRequest::new(&mut stream, sock).unwrap();
    let (_, _, headers, _, _, mut reader) = req.deconstruct();

    match parse_multipart_body(&mut reader, &headers, false) {
        Ok(nodes) => {

            assert_eq!(nodes.len(), 3);

            if let Node::Part(ref part) = nodes[0] {
                assert_eq!(part.body, b"{\r\n\
                                          \"id\": 15\r\n\
                                        }");
            } else {
                panic!("1st node of wrong type");
            }

            if let Node::File(ref filepart) = nodes[1] {
                assert_eq!(filepart.size, Some(30));
                assert_eq!(filepart.filename().unwrap().unwrap(), "image.gif");
                assert_eq!(filepart.content_type().unwrap(), mime!(Image/Gif));

                assert!(filepart.path.exists());
                assert!(filepart.path.is_file());
            } else {
                panic!("2nd node of wrong type");
            }

            if let Node::File(ref filepart) = nodes[2] {
                assert_eq!(filepart.size, Some(14));
                assert_eq!(filepart.filename().unwrap().unwrap(), "file.txt");
                assert!(filepart.content_type().is_none());

                assert!(filepart.path.exists());
                assert!(filepart.path.is_file());
            } else {
                panic!("3rd node of wrong type");
            }
        },
        Err(err) => panic!("{}", err),
    }
}

#[test]
fn mixed_parser() {

    let input = b"POST / HTTP/1.1\r\n\
                  Host: example.domain\r\n\
                  Content-Type: multipart/form-data; boundary=AaB03x\r\n\
                  Content-Length: 1000\r\n\
                  \r\n\
                  --AaB03x\r\n\
                  Content-Disposition: form-data; name=\"submit-name\"\r\n\
                  \r\n\
                  Larry\r\n\
                  --AaB03x\r\n\
                  Content-Disposition: form-data; name=\"files\"\r\n\
                  Content-Type: multipart/mixed; boundary=BbC04y\r\n\
                  \r\n\
                  --BbC04y\r\n\
                  Content-Disposition: file; filename=\"file1.txt\"\r\n\
                  \r\n\
                  ... contents of file1.txt ...\r\n\
                  --BbC04y\r\n\
                  Content-Disposition: file; filename=\"awesome_image.gif\"\r\n\
                  Content-Type: image/gif\r\n\
                  Content-Transfer-Encoding: binary\r\n\
                  \r\n\
                  ... contents of awesome_image.gif ...\r\n\
                  --BbC04y--\r\n\
                  --AaB03x--";

    let mut mock = MockStream::with_input(input);

    let mock: &mut NetworkStream = &mut mock;
    let mut stream = BufReader::new(mock);
    let sock: SocketAddr = "127.0.0.1:80".parse().unwrap();
    let req = HyperRequest::new(&mut stream, sock).unwrap();
    let (_, _, headers, _, _, mut reader) = req.deconstruct();

    match parse_multipart_body(&mut reader, &headers, false) {
        Ok(nodes) => {

            assert_eq!(nodes.len(), 2);

            if let Node::Part(ref part) = nodes[0] {
                let cd: &ContentDisposition = part.headers.get().unwrap();
                let cd_name: String = get_content_disposition_name(&cd).unwrap();
                assert_eq!(&*cd_name, "submit-name");
                assert_eq!(::std::str::from_utf8(&*part.body).unwrap(), "Larry");
            } else {
                panic!("1st node of wrong type");
            }

            if let Node::Multipart((ref headers, ref subnodes)) = nodes[1] {
                let cd: &ContentDisposition = headers.get().unwrap();
                let cd_name: String = get_content_disposition_name(&cd).unwrap();
                assert_eq!(&*cd_name, "files");

                assert_eq!(subnodes.len(), 2);

                if let Node::File(ref filepart) = subnodes[0] {
                    assert_eq!(filepart.size, Some(29));
                    assert_eq!(filepart.filename().unwrap().unwrap(), "file1.txt");
                    assert!(filepart.content_type().is_none());

                    assert!(filepart.path.exists());
                    assert!(filepart.path.is_file());
                } else {
                    panic!("1st subnode of wrong type");
                }

                if let Node::File(ref filepart) = subnodes[1] {
                    assert_eq!(filepart.size, Some(37));
                    assert_eq!(filepart.filename().unwrap().unwrap(), "awesome_image.gif");
                    assert_eq!(filepart.content_type().unwrap(), mime!(Image/Gif));

                    assert!(filepart.path.exists());
                    assert!(filepart.path.is_file());
                } else {
                    panic!("2st subnode of wrong type");
                }

            } else {
                panic!("2st node of wrong type");
            }
        },
        Err(err) => panic!("{}", err),
    }
}

#[inline]
fn get_content_disposition_name(cd: &ContentDisposition) -> Option<String> {
    if let Some(&DispositionParam::Ext(_, ref value)) = cd.parameters.iter()
        .find(|&x| match *x {
            DispositionParam::Ext(ref token,_) => &*token == "name",
            _ => false,
        })
    {
        Some(value.clone())
    } else {
        None
    }
}
