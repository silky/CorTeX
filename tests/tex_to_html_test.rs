// Copyright 2015 Deyan Ginev. See the LICENSE
// file at the top-level directory of this distribution.
//
// Licensed under the MIT license <LICENSE-MIT or http://opensource.org/licenses/MIT>.
// This file may not be copied, modified, or distributed
// except according to those terms.
extern crate cortex;
use cortex::backend::{Backend, TEST_DB_ADDRESS};
use cortex::data::{Corpus,Service, Task, TaskStatus};
use cortex::manager::{TaskManager};
use cortex::worker::{TexToHtmlWorker, Worker};
use cortex::importer::Importer;
use std::thread;
use std::str;
use std::process::Command;

#[test]
fn mock_tex_to_html() {
  // Check if we have latexmlc installed, skip otherwise:
  let which_result = Command::new("which").arg("latexmlc").output().unwrap().stdout;
  let latexmlc_path = str::from_utf8(&which_result).unwrap();
  if latexmlc_path.is_empty() {
    println!("latexmlc not installed, skipping test");
    return assert!(true);
  }
  // Initialize a corpus, import a single task, and enable a service on it
  let test_backend = Backend::testdb();
  assert!(test_backend.setup_task_tables().is_ok());
  
  let mock_corpus = test_backend.add(
    Corpus {
      id : None,
      name : "mock round-trip corpus".to_string(),
      path : "tests/data/".to_string(),
      complex : true,
    }).unwrap();
  let tex_to_html_service = test_backend.add(
    Service { 
      id : None,
      name : "tex_to_html".to_string(),
      version : 0.1,
      inputformat : "tex".to_string(),
      outputformat : "html".to_string(),
      inputconverter : Some("import".to_string()),
      complex : true
    }).unwrap();
  let mut abs_path = Importer::cwd();
  abs_path.push("tests/data/1206.5501/1206.5501.zip");
  let abs_entry = abs_path.to_str().unwrap().to_string();
  test_backend.add(
    Task {
      id : None,
      entry : abs_entry.clone(),
      serviceid : 1, // Import service always has id 1
      corpusid : mock_corpus.id.unwrap().clone(),
      status : TaskStatus::NoProblem.raw()
    }).unwrap();
  test_backend.add(
    Task {
      id : None,
      entry : abs_entry.clone(),
      serviceid : tex_to_html_service.id.unwrap().clone(),
      corpusid : mock_corpus.id.unwrap().clone(),
      status : TaskStatus::TODO.raw()
    }).unwrap();
  
  // Start up a ventilator/sink pair
  thread::spawn(move || {
    let manager = TaskManager {
      source_port : 5555,
      result_port : 5556,
      queue_size : 100000,
      message_size : 100,
      backend_address : TEST_DB_ADDRESS.clone().to_string()
    };
    assert!(manager.start().is_ok());
  });
  // Start up an tex to html worker
  let worker = TexToHtmlWorker::default();
  // Perform a single echo task 
  assert!(worker.start(Some(1)).is_ok());
  // Check round-trip success
}