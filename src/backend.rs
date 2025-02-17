// Copyright 2015 Deyan Ginev. See the LICENSE
// file at the top-level directory of this distribution.
//
// Licensed under the MIT license <LICENSE-MIT or http://opensource.org/licenses/MIT>.
// This file may not be copied, modified, or distributed
// except according to those terms.

//! ORM-like capabilities for high- and mid-level operations on the Task store
extern crate postgres;
extern crate rustc_serialize;
extern crate rand;

use postgres::{Connection, SslMode};
use postgres::error::Error;
use postgres::rows::{Rows};
use std::clone::Clone;
use std::collections::HashMap;
use regex::Regex;

use data::{CortexORM, Corpus, Service, Task, TaskReport, TaskStatus};

use rand::{thread_rng, Rng};

/// Provides an interface to the Postgres task store
pub struct Backend {
  /// the Postgres database `Connection`
  pub connection : Connection
}

/// By default, use a localhost-only cortex user/pass
pub static DEFAULT_DB_ADDRESS : &'static str = "postgres://cortex:cortex@localhost/cortex";
/// Similarly, use a cortex_tester user/pass for tests
pub static TEST_DB_ADDRESS : &'static str = "postgres://cortex_tester:cortex_tester@localhost/cortex_tester";
impl Default for Backend {
  fn default() -> Backend {
    Backend {
      connection: Connection::connect(DEFAULT_DB_ADDRESS.clone(), &SslMode::None).unwrap()
    }
  }
}

impl Backend {
  /// Constructs a new Task store representation from a Postgres DB address
  pub fn from_address(address : &str) -> Backend {
   Backend {
      connection: Connection::connect(address, &SslMode::None).unwrap()
    } 
  }
  /// Constructs the default Backend struct for testing
  pub fn testdb() -> Backend {
   Backend {
      connection: Connection::connect(TEST_DB_ADDRESS.clone(), &SslMode::None).unwrap()
    }
  }

  /// Instance methods

  /// Checks if the Task store has been initialized, heuristically, by trying to detect if the `init` service has been added.
  pub fn needs_init(&self) -> bool {
    match self.connection.prepare("SELECT * FROM services where name='init'") {
      Ok(init_check_query) => {
        match init_check_query.query(&[]) {
          Ok(rows) => {
            rows.len() == 0
          },
          _ => true
        }
      },
      _ => true
    }
  }
  /// Sets up the CorTeX tables and indexes, dropping existing infrastructure when applicable (hard reset)
  pub fn setup_task_tables(&self) -> postgres::Result<()> {
    let trans = try!(self.connection.transaction());
    // Tasks
    trans.execute("DROP TABLE IF EXISTS tasks;", &[]).unwrap();
    trans.execute("CREATE TABLE tasks (
      taskid BIGSERIAL PRIMARY KEY,
      serviceid INTEGER NOT NULL,
      corpusid INTEGER NOT NULL,
      entry char(200) NOT NULL,
      status INTEGER NOT NULL
    );", &[]).unwrap();
    trans.execute("create index entryidx on tasks(entry);", &[]).unwrap();
    trans.execute("create index serviceidx on tasks(serviceid);", &[]).unwrap();
    trans.execute("create index ok_index on tasks(status,serviceid,corpusid,taskid,entry) where status = -1;", &[]).unwrap();
    trans.execute("create index warning_index on tasks(status,serviceid,corpusid,taskid,entry) where status = -2;", &[]).unwrap();
    trans.execute("create index error_index on tasks(status,serviceid,corpusid,taskid,entry) where status = -3;", &[]).unwrap();
    trans.execute("create index fatal_index on tasks(status,serviceid,corpusid,taskid,entry) where status = -4;", &[]).unwrap();
    // Corpora
    trans.execute("DROP TABLE IF EXISTS corpora;", &[]).unwrap();
    trans.execute("CREATE TABLE corpora (
      corpusid SERIAL PRIMARY KEY,
      path varchar(200) NOT NULL,
      name varchar(200) NOT NULL,
      complex boolean NOT NULL
    );", &[]).unwrap();
    trans.execute("create index corpusnameidx on corpora(name);", &[]).unwrap();
    // Services
    trans.execute("DROP TABLE IF EXISTS services;", &[]).unwrap();
    trans.execute("CREATE TABLE services (
      serviceid SERIAL PRIMARY KEY,
      name varchar(200) NOT NULL,
      version real NOT NULL,
      inputformat varchar(20) NOT NULL,
      outputformat varchar(20) NOT NULL,
      inputconverter varchar(200),
      complex boolean NOT NULL,
      UNIQUE(name,version)
    );", &[]).unwrap();
    trans.execute("create index servicenameidx on services(name);", &[]).unwrap();
    // trans.execute("create index serviceiididx on services(iid);", &[]).unwrap();
    trans.execute("INSERT INTO services (name, version, inputformat,outputformat,complex)
           values('init',0.1, 'tex','tex', true);", &[]).unwrap();
    trans.execute("INSERT INTO services (name, version, inputformat,outputformat,complex)
           values('import',0.1, 'tex','tex', true);", &[]).unwrap();

    // Dependency Tables
    trans.execute("DROP TABLE IF EXISTS dependencies;", &[]).unwrap();
    trans.execute("CREATE TABLE dependencies (
      master INTEGER NOT NULL,
      foundation INTEGER NOT NULL,
      PRIMARY KEY (master, foundation)
    );", &[]).unwrap();
    trans.execute("create index masteridx on dependencies(master);", &[]).unwrap();
    trans.execute("create index foundationidx on dependencies(foundation);", &[]).unwrap();

    // Log Tables
    trans.execute("DROP TABLE if EXISTS logs", &[]).unwrap();
    trans.execute("CREATE TABLE logs (
      messageid BIGSERIAL PRIMARY KEY,
      taskid BIGINT NOT NULL,
      severity char(50),
      category char(50),
      what char(50),
      details varchar(2000)
    );", &[]).unwrap();

    // Note: Needed for efficient task rerun queries
    trans.execute("create index log_taskid on logs(taskid);", &[]).unwrap();
    // Note: to avoid a sequential scan on logs for all the report pages, the following 3 indexes are crucial:
    trans.execute("create index log_fatal_index on logs(severity,category,what,taskid) where severity = 'fatal';", &[]).unwrap();
    trans.execute("create index log_error_index on logs(severity,category,what,taskid) where severity = 'error';", &[]).unwrap();
    trans.execute("create index log_warning_index on logs(severity,category,what,taskid) where severity = 'warning';", &[]).unwrap();
    trans.set_commit();
    try!(trans.finish());
    Ok(())
  }

  /// Insert a vector of new `Task` tasks into the Task store
  /// For example, on import, or when a new service is activated on a corpus
  pub fn mark_imported(&self, tasks: &Vec<Task>) -> Result<(),Error> {
    let trans = try!(self.connection.transaction());
    for task in tasks {
      trans.execute("INSERT INTO tasks (entry,serviceid,corpusid,status) VALUES ($1,$2,$3,$4)",
        &[&task.entry, &task.serviceid, &task.corpusid, &task.status]).unwrap();
    }
    trans.set_commit();
    try!(trans.finish());
    Ok(())
  }

  /// Insert a vector of `TaskReport` reports into the Task store, also marking their tasks as completed with the correct status code.
  pub fn mark_done(&self, reports: &Vec<TaskReport>) -> Result<(),Error> {
    let trans = try!(self.connection.transaction());
    let insert_log_message = trans.prepare("INSERT INTO logs (taskid, severity, category, what, details) values($1,$2,$3,$4,$5)").unwrap();
    // let insert_log_message_details = trans.prepare("INSERT INTO logdetails (messageid, details) values(?,?)").unwrap();
    for report in reports.iter() {
      let taskid = report.task.id.unwrap();
      trans.execute("UPDATE tasks SET status=$1 WHERE taskid=$2",
        &[&report.status.raw(), &taskid]).unwrap();
      for message in &report.messages {
        if (message.severity == "info") || (message.severity == "status") {
          continue; // Skip info and status information, keep the DB small
        } else {
          // Warnings, Errors and Fatals will get added:
          insert_log_message.query(&[&taskid, 
            &message.severity, &message.category, &message.what, &message.details]).unwrap();
        }
      }
      // TODO: Update dependencies
    }
    trans.set_commit();
    try!(trans.finish());
    Ok(())
  }

  /// Given a complex selector, of a `Corpus`, `Service`, and the optional `severity`, `category` and `what`
  /// mark all matching tasks to be rerun
  pub fn mark_rerun(&self, corpus : &Corpus, service : &Service,
    severity: Option<String>, category: Option<String>, what: Option<String>) -> Result<(), Error> {

    let mut rng = thread_rng();
    let mark_rng: u16 = rng.gen();
    let mark : i32 = -1 * (mark_rng as i32);

    // First, mark as blocked all of the tasks in the chosen scope, using a special mark
    match severity {
      Some(severity) => {
        match category {
          Some(category) => {
            match what {
              Some(what) => { // All tasks in a "what" class
                try!(self.connection.execute(
                  "UPDATE tasks SET status=$1 where corpusid=$2 and serviceid=$3 and taskid in (select distinct(taskid) from logs where severity=$4 and category=$5 and what=$6)",
                  &[&mark, &corpus.id.unwrap(), &service.id.unwrap(), &severity, &category, &what])
                );
              },
              None => { // All tasks in a category
                try!(self.connection.execute(
                  "UPDATE tasks SET status=$1 where corpusid=$2 and serviceid=$3 and taskid in (select distinct(taskid) from logs where severity=$4 and category=$5)",
                  &[&mark, &corpus.id.unwrap(), &service.id.unwrap(), &severity, &category])
                );
              }
            };
          },
          None => { // All tasks in a certain status
            let status : i32 = TaskStatus::from_key(&severity).raw();
            try!(self.connection.execute(
              "UPDATE tasks SET status=$1 where corpusid=$2 and serviceid=$3 and status=$4",
              &[&mark, &corpus.id.unwrap(), &service.id.unwrap(), &status])
            );
          }
        }
      },
      None => { // Entire corpus
        try!(self.connection.execute("UPDATE tasks SET status=$1 where corpusid=$2 and serviceid=$3",
          &[&mark, &corpus.id.unwrap(), &service.id.unwrap()])
        );
      }
    };

    // Next, delete all logs for the blocked tasks.
    // Note that if we are using a negative blocking status, this query should get sped up via an "Index Scan using log_taskid on logs"
    try!(self.connection.execute(
      "DELETE from logs USING tasks WHERE logs.taskid=tasks.taskid and tasks.status=$1 and tasks.corpusid=$2 and tasks.serviceid=$3;",
      &[&mark, &corpus.id.unwrap(), &service.id.unwrap()])
    );

    // Lastly, switch all blocked tasks to "queued", and complete the rerun mark pass.
    try!(self.connection.execute(
      "UPDATE tasks set status=-5 where status=$1 and corpusid=$2 and serviceid=$3;",
      &[&mark, &corpus.id.unwrap(), &service.id.unwrap()])
    );
    Ok(())
  }

  /// Generic sync method, attempting to obtain the DB record for a given mock Task store datum
  /// applicable for any struct implementing the `CortexORM` trait
  /// (for example `Corpus`, `Service`, `Task`)
  pub fn sync<D: CortexORM + Clone>(&self, d: &D) -> Result<D, Error> {
    let synced = match d.get_id() {
      Some(_) => {
        try!(d.select_by_id(&self.connection))
      },
      None => {
        try!(d.select_by_key(&self.connection))
      }
    };
    match synced {
      Some(synced_d) => Ok(synced_d),
      None => Ok(d.clone())
    }
  }

  /// Generic delete method, attempting to delete the DB record for a given Task store datum
  /// applicable for any struct implementing the `CortexORM` trait
  /// (for example `Corpus`, `Service`, `Task`)
  pub fn delete<D: CortexORM + Clone>(&self, d: &D) -> Result<(), Error> {
    let d_checked = try!(self.sync(d));
    match d_checked.get_id() {
      Some(_) => d.delete(&self.connection),
      None => Ok(()) // No ID means we don't really know what to delete.
    }
  }

  /// Generic addition method, attempting to insert in the DB a Task store datum
  /// applicable for any struct implementing the `CortexORM` trait
  /// (for example `Corpus`, `Service`, `Task`)
  ///
  /// Note: Overwrites if the entry already existed.
  pub fn add<D: CortexORM + Clone>(&self, d: D) -> Result<D, Error> {
    let d_checked = try!(self.sync(&d));
    match d_checked.get_id() {
      Some(_) => {
        // If this data item existed - delete any remnants of it
        try!(self.delete(&d_checked));
      },
      None => {} // New, we can add it safely
    };
    // Add data item to the DB:
    try!(d.insert(&self.connection));
    let d_final = try!(self.sync(&d));
    Ok(d_final)
  }

  /// Fetches no more than `limit` queued tasks for a given `Service`
  pub fn fetch_tasks(&self, service: &Service, limit : usize) -> Result<Vec<Task>, Error> {
    match service.id { 
      Some(_) => {}
      None => {return Ok(Vec::new())}
    };
    let mut rng = thread_rng();
    let mark: u16 = rng.gen();

    // TODO: Concurrent use needs to add "and pg_try_advisory_xact_lock(taskid)" in the proper fashion
    //       But we need to be careful that the LIMIT takes place before the lock, which is why I removed it for now.
    let stmt = try!(self.connection.prepare(
      "UPDATE tasks t SET status = $1 FROM (
          SELECT * FROM tasks WHERE serviceid = $2 and status = $3
          LIMIT $4
          FOR UPDATE
        ) subt
        WHERE t.taskid = subt.taskid
        RETURNING t.taskid,t.entry,t.serviceid,t.corpusid,t.status;"));
    let rows = try!(stmt.query(&[&(mark as i32), &service.id.unwrap(), &TaskStatus::TODO.raw(), &(limit as i64)]));
    Ok(rows.iter().map(|row| Task::from_row(row)).collect::<Vec<_>>())
  }

  /// Globally resets any "in progress" tasks back to "queued".
  /// Particularly useful for dispatcher restarts, when all "in progress" tasks need to be invalidated
  pub fn clear_limbo_tasks(&self) -> Result<(), Error> {
    try!(self.connection.execute("UPDATE tasks SET status=$1 WHERE status > $2", &[&TaskStatus::TODO.raw(), &TaskStatus::NoProblem.raw(),]));
    Ok(())
  }

  /// Activates an existing service on a given corpus path
  pub fn register_service(&self, service: Service, corpus_path: String) -> Result<(),Error> {
    let corpus_placeholder = Corpus {
      id : None,
      path : corpus_path.clone(),
      name : corpus_path,
      complex : true
    };
    let corpus = self.sync(&corpus_placeholder).unwrap();
    let corpusid = corpus.id.unwrap();
    let serviceid = service.id.unwrap();
    let todo_raw = TaskStatus::TODO.raw();

    try!(self.connection.execute("DELETE from tasks where serviceid=$1 AND corpusid=$2", &[&serviceid, &corpusid]));
    let task_entries_query = try!(self.connection.prepare("SELECT entry from tasks where serviceid=2 AND corpusid=$1"));
    let task_entries = try!(task_entries_query.query(&[&corpus.id.unwrap()]));
    let trans = try!(self.connection.transaction());   
    for task_entry in task_entries.iter() {
      let entry : String = task_entry.get(0);
      trans.execute("INSERT INTO tasks (entry,serviceid,corpusid, status) VALUES ($1,$2,$3,$4)",
        &[&entry, &serviceid, &corpusid, &todo_raw]).unwrap();
    }
    trans.set_commit();
    try!(trans.finish());
    Ok(())
 }

  /// Returns a vector of currently available corpora in the Task store
  pub fn corpora(&self) -> Vec<Corpus> {
    let mut corpora = Vec::new();
    match self.connection.prepare("SELECT corpusid,name,path,complex FROM corpora order by name") {
      Ok(select_query) => {
        match select_query.query(&[]) {
          Ok(rows) => {
            for row in rows.iter() {
              corpora.push(Corpus::from_row(row));
            }
          },
          _ => {}
        }
      }
      _ => {}
    }
    return corpora;
  }

  /// Provides a progress report, grouped by severity, for a given `Corpus` and `Service` pair
  pub fn progress_report<'report>(&self, c : &Corpus, s : &Service) -> HashMap<String, f64> {
    let mut stats_hash : HashMap<String, f64> = HashMap::new();
    for status_key in TaskStatus::keys().into_iter() {
      stats_hash.insert(status_key,0.0);
    }
    stats_hash.insert("total".to_string(),0.0);
    match self.connection.prepare("select status,count(*) as status_count from tasks where serviceid=$1 and corpusid=$2 group by status order by status_count desc;") {
      Ok(select_query) => {
        match select_query.query(&[&s.id.unwrap(), &c.id.unwrap()]) {
          Ok(rows) => {
            for row in rows.iter() {
              let status_code = TaskStatus::from_raw(row.get(0)).to_key();
              let count : i64 = row.get(1);
              {
                let status_frequency = stats_hash.entry(status_code).or_insert(0.0);
                *status_frequency += count as f64;
              }
              let total_frequency = stats_hash.entry("total".to_string()).or_insert(0.0);
              *total_frequency += count as f64;
            }
          },
          _ => {}
        }
      }
      _ => {}
    }
    Backend::aux_stats_compute_percentages(&mut stats_hash, None);
    stats_hash
  }

  /// Given a complex selector, of a `Corpus`, `Service`, and the optional `severity`, `category` and `what`,
  /// Provide a progress report at the chosen granularity
  pub fn task_report<'report>(&self, c : &Corpus, s : &Service,
    severity: Option<String>, category: Option<String>, what: Option<String>) -> Vec<HashMap<String, String>> {
    match severity {
      Some(severity_name) => {
        let raw_status = TaskStatus::from_key(&severity_name).raw();
        if severity_name == "no_problem" {
        match self.connection.prepare("select entry,taskid from tasks where serviceid=$1 and corpusid=$2 and status=$3 limit 100;") {
          Ok(select_query) => match select_query.query(&[&s.id.unwrap(), &c.id.unwrap(), &raw_status]) {
            Ok(entry_rows) => {
              let entry_name_regex = Regex::new(r"^.+/(.+)\..+$").unwrap();
              let mut entries = Vec::new();
              for row in entry_rows {
                let mut entry_map = HashMap::new();
                let entry_fixedwidth : String = row.get(0);
                let entry_taskid : i64 = row.get(1);
                let entry = entry_fixedwidth.trim_right().to_string();
                let entry_name = entry_name_regex.replace(&entry,"$1");
                
                entry_map.insert("entry".to_string(),entry);
                entry_map.insert("entry_name".to_string(),entry_name);
                entry_map.insert("entry_taskid".to_string(),entry_taskid.to_string());
                entry_map.insert("details".to_string(),"OK".to_string());
                entries.push(entry_map);
              }
              entries},
            _ => Vec::new()
          },
          _ => Vec::new()
        }}
        else {
          let total_count_query = self.connection.prepare("select count(*) from tasks WHERE serviceid=$1 and corpusid=$2;").unwrap();
          let total_tasks : i64 = match total_count_query.query(&[&s.id.unwrap(), &c.id.unwrap()]) {
            Err(_) => 0,
            Ok(count) => count.get(0).get(0)
          };
          match category {
          // using ::int4 since the rust postgresql wrapper can't map Numeric into Rust yet, but it is fine with bigint (as i64)
          None => match self.connection.prepare("select category, count(*) as task_count, sum(total_counts::int4) from (
              select logs.category, logs.taskid, count(*) as total_counts from tasks LEFT OUTER JOIN logs ON (tasks.taskid=logs.taskid) WHERE serviceid=$1 and corpusid=$2 and status=$3 and severity=$4
               group by logs.category, logs.taskid) as tmp GROUP BY category ORDER BY task_count desc;") {
            Ok(select_query) => {
              match select_query.query(&[&s.id.unwrap(), &c.id.unwrap(), &raw_status, &severity_name]) {
                Ok(category_rows) => {
                  // How many tasks total in this category?
                  match self.connection.prepare("select count(*) from tasks, logs where tasks.taskid=logs.taskid and serviceid=$1 and corpusid=$2 and status=$3 and severity=$4;") {
                  Ok(total_query) => {
                    match total_query.query(&[&s.id.unwrap(), &c.id.unwrap(), &raw_status, &severity_name]) {
                      Ok(total_rows) => {
                        let total_messages : i64 = total_rows.get(0).get(0);
                        Backend::aux_task_rows_stats(category_rows, total_tasks, total_messages)
                      },
                      _ => Vec::new()
                    }
                  },
                  _ => Vec::new()
                  }
                },
                _ => Vec::new()
              }
            },
            _ => Vec::new(),
          },
          Some(category_name) => match what {
            // using ::int4 since the rust postgresql wrapper can't map Numeric into Rust yet, but it is fine with bigint (as i64)
            None => match self.connection.prepare("select what, count(*) as task_count, sum(total_counts::int4) from (
              select logs.what, logs.taskid, count(*) as total_counts from tasks LEFT OUTER JOIN logs ON (tasks.taskid=logs.taskid)
              WHERE serviceid=$1 and corpusid=$2 and status=$3 and severity=$4 and category=$5
              GROUP BY logs.what, logs.taskid) as tmp GROUP BY what ORDER BY task_count desc;") {
              Ok(select_query) => match select_query.query(&[&s.id.unwrap(), &c.id.unwrap(), &raw_status, &severity_name, &category_name]) {
                Ok(what_rows) => {
                  // How many tasks total in this category?
                  match self.connection.prepare("select count(*) from tasks, logs where tasks.taskid=logs.taskid and serviceid=$1 and corpusid=$2 and status=$3 and severity=$4 and category=$5;") {
                  Ok(total_query) => {
                    match total_query.query(&[&s.id.unwrap(), &c.id.unwrap(), &raw_status, &severity_name, &category_name]) {
                      Ok(total_rows) => {
                        let total_messages : i64 = total_rows.get(0).get(0);
                        Backend::aux_task_rows_stats(what_rows, total_tasks, total_messages)
                      },
                      _ => Vec::new()
                    }},
                  _ => Vec::new()
                  }
                },
                _ => Vec::new()
              },
              _ => Vec::new()
            },
            Some(what_name) => match self.connection.prepare("select tasks.taskid, tasks.entry, logs.details from tasks, logs where tasks.taskid=logs.taskid and serviceid=$1 and corpusid=$2 and status=$3 and severity=$4 and category=$5 and what=$6 limit 100;") {
            Ok(select_query) => match select_query.query(&[&s.id.unwrap(), &c.id.unwrap(), &raw_status,&severity_name, &category_name,&what_name]) {
              Ok(entry_rows) => {
                let entry_name_regex = Regex::new(r"^.+/(.+)\..+$").unwrap();
                let mut entries = Vec::new();
                for row in entry_rows {
                  let mut entry_map = HashMap::new();
                  let entry_taskid : i64 = row.get(0);
                  let entry_fixedwidth : String = row.get(1);
                  let details : String = row.get(2);
                  let entry = entry_fixedwidth.trim_right().to_string();
                  let entry_name = entry_name_regex.replace(&entry,"$1");
                  // TODO: Also use url-escape
                  entry_map.insert("entry".to_string(),entry);
                  entry_map.insert("entry_name".to_string(),entry_name);
                  entry_map.insert("entry_taskid".to_string(),entry_taskid.to_string());
                  entry_map.insert("details".to_string(),details);
                  entries.push(entry_map);
                }
                entries
              },
              _ => Vec::new()
            },
            _ => Vec::new()
            }
          }
        }}
      },
      None => Vec::new()
    }
  }
  fn aux_stats_compute_percentages(stats_hash : &mut HashMap<String, f64>, total_given : Option<f64>) {
     //Compute percentages, now that we have a total
    let total : f64 = 1.0_f64.max(match total_given {
      None => {
          let total_entry = stats_hash.get_mut("total").unwrap();
          (*total_entry).clone()
        },
      Some(total_num) => total_num
    });
    let stats_keys = stats_hash.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>();
    for stats_key in stats_keys {
      {
        let key_percent_value : f64 = 100.0 * (*stats_hash.get_mut(&stats_key).unwrap() as f64 / total as f64);
        let key_percent_rounded : f64 = (key_percent_value * 100.0).round() as f64 / 100.0;
        let key_percent_name = stats_key + "_percent";
        stats_hash.insert(key_percent_name, key_percent_rounded);
      }
    }
  }
  fn aux_task_rows_stats(rows : Rows, total_tasks : i64, total_messages : i64) -> Vec<HashMap<String,String>>{
    let mut report = Vec::new();

    for row in rows.iter() {
      let stat_type_fixedwidth : String = row.get(0);
      let stat_type : String = stat_type_fixedwidth.trim_right().to_string();
      let stat_tasks : i64 = row.get(1);
      let stat_messages : i64 = row.get(2);
      let mut stats_hash : HashMap<String, String> = HashMap::new();
      stats_hash.insert("name".to_string(),stat_type);
      stats_hash.insert("tasks".to_string(), stat_tasks.to_string());
      stats_hash.insert("messages".to_string(), stat_messages.to_string());

      let tasks_percent_value : f64 = 100.0 * (stat_tasks  as f64 / total_tasks as f64);
      let tasks_percent_rounded : f64 = (tasks_percent_value * 100.0).round() as f64 / 100.0;
      stats_hash.insert("tasks_percent".to_string(), tasks_percent_rounded.to_string());
      let messages_percent_value : f64 = 100.0 * (stat_messages  as f64 / total_messages as f64);
      let messages_percent_rounded : f64 = (messages_percent_value * 100.0).round() as f64 / 100.0;
      stats_hash.insert("messages_percent".to_string(), messages_percent_rounded.to_string());

      report.push(stats_hash);
    }
    // Append the total to the end of the report:
    let mut total_hash = HashMap::new();
    total_hash.insert("name".to_string(),"total".to_string());
    total_hash.insert("tasks".to_string(),total_tasks.to_string());
    total_hash.insert("tasks_percent".to_string(),"100".to_string());
    total_hash.insert("messages".to_string(),total_messages.to_string());
    total_hash.insert("messages_percent".to_string(),"100".to_string());
    report.push(total_hash);


    report
  }

}