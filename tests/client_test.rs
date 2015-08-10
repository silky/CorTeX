// Copyright 2015 Deyan Ginev. See the LICENSE
// file at the top-level directory of this distribution.
//
// Licensed under the MIT license <LICENSE-MIT or http://opensource.org/licenses/MIT>.
// This file may not be copied, modified, or distributed
// except according to those terms.
extern crate cortex;
use cortex::backend::Backend;
use cortex::data::{Corpus,Service, Task, TaskStatus};

#[test]
fn mock_round_trip() {
  // Initialize a corpus, import a single task, and enable a service on it
  let test_backend = Backend::testdb();
  assert!(test_backend.setup_task_tables().is_ok());
  
  let mock_corpus = test_backend.add_corpus(
    Corpus {
      id : None,
      name : "mock round-trip corpus".to_string(),
      path : "tests/data/".to_string(),
      complex : true,
    }).unwrap();
  let echo_service = test_backend.add_service(
    Service { 
      id : None,
      name : "echo service".to_string(),
      version : 0.1,
      inputformat : "tex".to_string(),
      outputformat : "tex".to_string(),
      inputconverter : Some("import".to_string()),
      complex : true
    }).unwrap();
  let import_task = test_backend.add_task(
    Task {
      id : None,
      entry : "tests/data/1508.01222/1508.01222.zip".to_string(),
      serviceid : 1, // Import service always has id 1
      corpusid : mock_corpus.id.unwrap().clone(),
      status : TaskStatus::NoProblem.raw()
    }).unwrap();
  let echo_task = test_backend.add_task(
    Task {
      id : None,
      entry : "tests/data/1508.01222/1508.01222.zip".to_string(),
      serviceid : echo_service.id.unwrap().clone(),
      corpusid : mock_corpus.id.unwrap().clone(),
      status : TaskStatus::TODO.raw()
    }).unwrap();
  // Start up a client


  // Start up an echo worker

  // Check round-trip success
}