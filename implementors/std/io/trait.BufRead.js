(function() {var implementors = {};
implementors['bufstream'] = ["impl&lt;S: <a class='trait' href='https://doc.rust-lang.org/nightly/std/io/trait.Read.html' title='std::io::Read'>Read</a> + <a class='trait' href='https://doc.rust-lang.org/nightly/std/io/trait.Write.html' title='std::io::Write'>Write</a>&gt; <a class='trait' href='https://doc.rust-lang.org/nightly/std/io/trait.BufRead.html' title='std::io::BufRead'>BufRead</a> for <a class='struct' href='bufstream/struct.BufStream.html' title='bufstream::BufStream'>BufStream</a>&lt;S&gt;",];implementors['openssl'] = [];implementors['tempfile'] = [];implementors['postgres'] = ["impl&lt;'a&gt; <a class='trait' href='https://doc.rust-lang.org/nightly/std/io/trait.BufRead.html' title='std::io::BufRead'>BufRead</a> for <a class='struct' href='postgres/stmt/struct.CopyOutReader.html' title='postgres::stmt::CopyOutReader'>CopyOutReader</a>&lt;'a&gt;",];implementors['nickel'] = ["impl&lt;R&gt; <a class='trait' href='https://doc.rust-lang.org/nightly/std/io/trait.BufRead.html' title='std::io::BufRead'>BufRead</a> for <a class='struct' href='hyper/buffer/struct.BufReader.html' title='hyper::buffer::BufReader'>BufReader</a>&lt;R&gt; <span class='where'>where R: <a class='trait' href='https://doc.rust-lang.org/nightly/std/io/trait.Read.html' title='std::io::Read'>Read</a></span>",];

            if (window.register_implementors) {
                window.register_implementors(implementors);
            } else {
                window.pending_implementors = implementors;
            }
        
})()
