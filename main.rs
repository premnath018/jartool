use clap::{Arg, Command};
use colored::*;
use csv::Writer;
use rayon::prelude::*;
use regex::Regex;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use walkdir::WalkDir;
use zip::ZipArchive;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub file_location: String,
    pub line_number: Option<usize>,
    pub line_content: String,
    pub match_type: String,
}

#[derive(Debug, Default)]
pub struct SearchStats {
    pub total_jars: usize,
    pub total_zip_files: usize,
    pub total_class_files: usize,
    pub total_java_files: usize,
    pub total_other_files: usize,
    pub matches_found: usize,
    pub files_processed: usize,
    pub elapsed_time: Duration,
}

#[derive(Debug)]
pub struct JarTool {
    stats: Arc<Mutex<SearchStats>>,
    results: Arc<Mutex<Vec<SearchResult>>>,
    verbose: bool,
    size_threshold: u64,
    parallel_jobs: usize,
    excludes: HashSet<String>,
    mini_mode: bool,
    unique_files: Arc<Mutex<HashSet<String>>>,
}

impl JarTool {
    pub fn new(verbose: bool, size_threshold: u64, parallel_jobs: Option<usize>, excludes: Vec<String>, mini_mode: bool) -> Self {
        let jobs = parallel_jobs.unwrap_or_else(|| num_cpus::get());
        rayon::ThreadPoolBuilder::new()
            .num_threads(jobs)
            .build_global()
            .expect("Failed to build thread pool");

        let exclude_set: HashSet<String> = excludes.into_iter().collect();

        Self {
            stats: Arc::new(Mutex::new(SearchStats::default())),
            results: Arc::new(Mutex::new(Vec::new())),
            verbose,
            size_threshold,
            parallel_jobs: jobs,
            excludes: exclude_set,
            mini_mode,
            unique_files: Arc::new(Mutex::new(HashSet::new())),
        }
    }


    fn should_exclude_path(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        for exclude in &self.excludes {
            if path_str.contains(exclude) {
                self.log_verbose(&format!("Excluding path: {} (matches: {})", path_str, exclude));
                return true;
            }
        }
        false
    }

    fn log_verbose(&self, msg: &str) {
        if self.verbose {
            eprintln!("{} {}", "[DEBUG]".blue(), msg);
        }
    }

    fn update_stats<F>(&self, updater: F)
    where
        F: FnOnce(&mut SearchStats),
    {
        if let Ok(mut stats) = self.stats.lock() {
            updater(&mut stats);
        }
    }

    fn add_result(&self, result: SearchResult) {
        if self.mini_mode {
            // In mini mode, only add unique file locations
            let file_location = result.file_location.clone();
            if let Ok(mut unique_files) = self.unique_files.lock() {
                if unique_files.insert(file_location.clone()) {
                    // This is a new file, add a simplified result
                    let mini_result = SearchResult {
                        file_location,
                        line_number: None,
                        line_content: "Found matches".to_string(),
                        match_type: result.match_type,
                    };
                    if let Ok(mut results) = self.results.lock() {
                        results.push(mini_result);
                    }
                }
            }
        } else {
            // Normal mode, add all results
            if let Ok(mut results) = self.results.lock() {
                results.push(result);
            }
        }
        self.update_stats(|stats| stats.matches_found += 1);
    }

    pub fn search_exact_class(&self, query: &str, search_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        self.log_verbose(&format!("Starting exact class search for: {}", query));
        let start_time = Instant::now();

        let jar_files = self.find_archive_files(search_dir, &["jar"])?;
        self.update_stats(|stats| stats.total_jars = jar_files.len());

        println!("{} Found {} JAR files to process", "INFO".green(), jar_files.len());

        jar_files.par_iter().for_each(|jar_path| {
            self.search_class_in_jar(jar_path, query, true);
        });

        self.update_stats(|stats| stats.elapsed_time = start_time.elapsed());
        Ok(())
    }

    pub fn search_class_substring(&self, query: &str, search_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        self.log_verbose(&format!("Starting class substring search for: {}", query));
        let start_time = Instant::now();

        let jar_files = self.find_archive_files(search_dir, &["jar"])?;
        self.update_stats(|stats| stats.total_jars = jar_files.len());

        println!("{} Found {} JAR files to process", "INFO".green(), jar_files.len());

        jar_files.par_iter().for_each(|jar_path| {
            self.search_class_in_jar(jar_path, query, false);
        });

        self.update_stats(|stats| stats.elapsed_time = start_time.elapsed());
        Ok(())
    }

    pub fn search_package(&self, package: &str, search_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        self.log_verbose(&format!("Starting package search for: {}", package));
        let start_time = Instant::now();

        let jar_files = self.find_archive_files(search_dir, &["jar"])?;
        self.update_stats(|stats| stats.total_jars = jar_files.len());

        let package_path = package.replace('.', "/");
        
        jar_files.par_iter().for_each(|jar_path| {
            self.search_package_in_jar(jar_path, &package_path);
        });

        self.update_stats(|stats| stats.elapsed_time = start_time.elapsed());
        Ok(())
    }

    pub fn search_content(&self, pattern: &str, search_dir: &Path, file_types: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
        self.log_verbose(&format!("Starting content search for: {}", pattern));
        let start_time = Instant::now();

        let regex = Regex::new(pattern)?;
        let jar_files = self.find_archive_files(search_dir, &["jar"])?;
        self.update_stats(|stats| stats.total_jars = jar_files.len());

        println!("{} Found {} JAR files to process", "INFO".green(), jar_files.len());

        jar_files.par_iter().for_each(|jar_path| {
            self.search_content_in_jar(jar_path, &regex, file_types);
        });

        self.update_stats(|stats| stats.elapsed_time = start_time.elapsed());
        Ok(())
    }

    pub fn search_java_files(&self, pattern: &str, search_dir: &Path, content_search: bool) -> Result<(), Box<dyn std::error::Error>> {
        self.log_verbose(&format!("Starting Java file search for: {}", pattern));
        let start_time = Instant::now();

        let regex = if content_search {
            Some(Regex::new(pattern)?)
        } else {
            Some(Regex::new(&format!(".*{}.*", regex::escape(pattern)))?)
        };

        let java_files: Vec<PathBuf> = WalkDir::new(search_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "java"))
            .map(|e| e.path().to_path_buf())
            .collect();

        self.update_stats(|stats| stats.total_java_files = java_files.len());
        println!("{} Found {} Java files to process", "INFO".green(), java_files.len());

        if content_search {
            java_files.par_iter().for_each(|java_path| {
                if let Some(ref regex) = regex {
                    self.search_content_in_file(java_path, regex);
                }
            });
        } else {
            for java_path in &java_files {
                if let Some(filename) = java_path.file_name() {
                    if let Some(ref regex) = regex {
                        if regex.is_match(&filename.to_string_lossy()) {
                            let result = SearchResult {
                                file_location: java_path.display().to_string(),
                                line_number: None,
                                line_content: "Java file name match".to_string(),
                                match_type: "java_filename".to_string(),
                            };
                            self.add_result(result);
                        }
                    }
                }
            }
        }

        self.update_stats(|stats| stats.elapsed_time = start_time.elapsed());
        Ok(())
    }

fn search_content_in_all_files(&self, file_path: &Path, regex: &Regex) {
    if !self.should_process_file(file_path) {
        return;
    }

    let file_ext = file_path.extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "no_extension".to_string());

    self.log_verbose(&format!("Processing {} file: {}", file_ext, file_path.display()));

    // Try to read as text first
    if let Ok(file) = File::open(file_path) {
        let reader = BufReader::new(file);
        let mut found_text_match = false;
        
        // First attempt: read as UTF-8 text
        for (line_num, line_result) in reader.lines().enumerate() {
            match line_result {
                Ok(line) => {
                    if regex.is_match(&line) {
                        let result = SearchResult {
                            file_location: file_path.display().to_string(),
                            line_number: Some(line_num + 1),
                            line_content: line.trim().to_string(),
                            match_type: self.get_file_type(file_path),
                        };
                        self.add_result(result);
                        found_text_match = true;
                    }
                },
                Err(_) => {
                    // If we encounter a read error (likely binary or encoding issue), 
                    // try binary search for remaining content
                    if !found_text_match {
                        self.log_verbose(&format!("Text read failed for {}, trying binary search", file_path.display()));
                        self.search_binary_file(file_path, regex);
                    }
                    break;
                }
            }
        }
        
        self.update_stats(|stats| stats.files_processed += 1);
    } else {
        self.log_verbose(&format!("Failed to open file: {}", file_path.display()));
    }
}



fn search_binary_file(&self, file_path: &Path, regex: &Regex) {
    if let Ok(mut file) = File::open(file_path) {
        let mut buffer = Vec::new();
        if file.read_to_end(&mut buffer).is_ok() {
            // Extract strings from binary data (similar to strings command)
            let mut current_string = String::new();
            let mut in_string = false;
            
            for &byte in &buffer {
                if byte.is_ascii_graphic() || byte == b' ' || byte == b'\t' {
                    current_string.push(byte as char);
                    in_string = true;
                } else {
                    if in_string && current_string.len() >= 4 {
                        if regex.is_match(&current_string) {
                            let result = SearchResult {
                                file_location: file_path.display().to_string(),
                                line_number: None,
                                line_content: current_string.clone(),
                                match_type: format!("{}_binary", self.get_file_type(file_path)),
                            };
                            self.add_result(result);
                        }
                    }
                    current_string.clear();
                    in_string = false;
                }
            }
            
            // Check final string
            if in_string && current_string.len() >= 4 && regex.is_match(&current_string) {
                let result = SearchResult {
                    file_location: file_path.display().to_string(),
                    line_number: None,
                    line_content: current_string,
                    match_type: format!("{}_binary", self.get_file_type(file_path)),
                };
                self.add_result(result);
            }
        }
    }
}

pub fn master_search(&self, pattern: &str, search_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    println!("{} Starting master search mode for: {}", "MASTER".yellow().bold(), pattern);
    if self.mini_mode {
        println!("{} Mini mode: showing unique files only", "MODE".purple());
    }
    println!("{} Processing ALL file types (.properties, .bat, .conf, .xml, etc.)", "INFO".green());
    
    let start_time = Instant::now();
    let regex = Regex::new(pattern)?;

    // Find all types of files with exclusion filtering
    let all_files: Vec<PathBuf> = WalkDir::new(search_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| !self.should_exclude_path(e.path())) // Add exclusion filter
        .map(|e| e.path().to_path_buf())
        .collect();

    let mut jar_files = Vec::new();
    let mut zip_files = Vec::new();
    let mut java_files = Vec::new();
    let mut config_files = Vec::new();
    let mut script_files = Vec::new();
    let mut xml_files = Vec::new();
    let mut text_files = Vec::new();
    let mut other_files = Vec::new();

    // Categorize files by type for better reporting
    for file in all_files {
        if let Some(ext) = file.extension() {
            match ext.to_str() {
                Some("jar") => jar_files.push(file),
                Some("zip") | Some("war") | Some("ear") => zip_files.push(file),
                Some("java") => java_files.push(file),
                Some("properties") | Some("conf") | Some("config") | Some("cfg") | Some("ini") => config_files.push(file),
                Some("bat") | Some("cmd") | Some("sh") | Some("ps1") | Some("py") | Some("rb") => script_files.push(file),
                Some("xml") | Some("xsd") | Some("xsl") | Some("xslt") => xml_files.push(file),
                Some("txt") | Some("md") | Some("log") | Some("yaml") | Some("yml") | Some("json") => text_files.push(file),
                _ => other_files.push(file),
            }
        } else {
            // Process files without extensions too
            other_files.push(file);
        }
    }

    // Combine all non-archive files for processing
    let mut all_other_files = Vec::new();
    all_other_files.extend(config_files.iter().cloned());
    all_other_files.extend(script_files.iter().cloned());
    all_other_files.extend(xml_files.iter().cloned());
    all_other_files.extend(text_files.iter().cloned());
    all_other_files.extend(other_files.iter().cloned());

    self.update_stats(|stats| {
        stats.total_jars = jar_files.len();
        stats.total_zip_files = zip_files.len();
        stats.total_java_files = java_files.len();
        stats.total_other_files = all_other_files.len();
    });

    println!("{} File analysis:", "INFO".green());
    println!("  JAR files: {}", jar_files.len());
    println!("  ZIP files: {}", zip_files.len());
    println!("  Java files: {}", java_files.len());
    println!("  Config files (.properties, .conf, .ini): {}", config_files.len());
    println!("  Script files (.bat, .sh, .py, etc.): {}", script_files.len());
    println!("  XML files (.xml, .xsd, etc.): {}", xml_files.len());
    println!("  Text files (.txt, .json, .yaml, etc.): {}", text_files.len());
    println!("  Other files: {}", other_files.len());
    println!("  {} Total files to process: {}", "TOTAL".cyan(), 
        jar_files.len() + zip_files.len() + java_files.len() + all_other_files.len());

    // Search in JAR files
    if !jar_files.is_empty() {
        println!("{} Searching in JAR files...", "PHASE".cyan());
        jar_files.par_iter().for_each(|jar_path| {
            self.search_content_in_jar(jar_path, &regex, &["*"]);
        });
    }

    // Search in ZIP files
    if !zip_files.is_empty() {
        println!("{} Searching in ZIP files...", "PHASE".cyan());
        zip_files.par_iter().for_each(|zip_path| {
            self.search_content_in_zip(zip_path, &regex);
        });
    }

    // Search in Java files
    if !java_files.is_empty() {
        println!("{} Searching in Java files...", "PHASE".cyan());
        java_files.par_iter().for_each(|java_path| {
            self.search_content_in_file(java_path, &regex);
        });
    }

    // Search in ALL other files (config, scripts, XML, text, etc.)
    if !all_other_files.is_empty() {
        println!("{} Searching in configuration, script, and other files (.properties, .bat, .conf, .xml, etc.)...", "PHASE".cyan());
        all_other_files.par_iter().for_each(|file_path| {
            self.search_content_in_all_files(file_path, &regex);
        });
    }

    self.update_stats(|stats| stats.elapsed_time = start_time.elapsed());
    println!("{} Master search completed!", "SUCCESS".green());
    Ok(())
}
    fn search_class_in_jar(&self, jar_path: &Path, query: &str, exact_match: bool) {
        if !self.should_process_file(jar_path) {
            return;
        }

        self.log_verbose(&format!("Processing JAR: {}", jar_path.display()));

        if let Ok(file) = File::open(jar_path) {
            if let Ok(mut archive) = ZipArchive::new(file) {
                let mut class_count = 0;
                
                for i in 0..archive.len() {
                    if let Ok(file_in_zip) = archive.by_index(i) {
                        let file_name = file_in_zip.name();
                        
                        if file_name.ends_with(".class") {
                            class_count += 1;
                            
                            let class_name = file_name
                                .strip_suffix(".class")
                                .unwrap_or(file_name)
                                .replace('/', ".");
                            
                            let matches = if exact_match {
                                class_name.ends_with(&format!(".{}", query)) || class_name == query
                            } else {
                                class_name.contains(query)
                            };
                            
                            if matches {
                                let result = SearchResult {
                                    file_location: format!("{}:{}", jar_path.display(), file_name),
                                    line_number: None,
                                    line_content: class_name,
                                    match_type: "class".to_string(),
                                };
                                self.add_result(result);
                            }
                        }
                    }
                }
                
                self.update_stats(|stats| {
                    stats.files_processed += 1;
                    stats.total_class_files += class_count;
                });
            }
        }
    }

    fn search_package_in_jar(&self, jar_path: &Path, package_path: &str) {
        if !self.should_process_file(jar_path) {
            return;
        }

        if let Ok(file) = File::open(jar_path) {
            if let Ok(mut archive) = ZipArchive::new(file) {
                for i in 0..archive.len() {
                    if let Ok(file_in_zip) = archive.by_index(i) {
                        let file_name = file_in_zip.name();
                        
                        if file_name.ends_with(".class") && file_name.starts_with(package_path) {
                            let class_name = file_name
                                .strip_suffix(".class")
                                .unwrap_or(file_name)
                                .replace('/', ".");
                            
                            let result = SearchResult {
                                file_location: format!("{}:{}", jar_path.display(), file_name),
                                line_number: None,
                                line_content: class_name,
                                match_type: "package".to_string(),
                            };
                            self.add_result(result);
                        }
                    }
                }
                self.update_stats(|stats| stats.files_processed += 1);
            }
        }
    }

    fn search_content_in_jar(&self, jar_path: &Path, regex: &Regex, file_types: &[&str]) {
        if !self.should_process_file(jar_path) {
            return;
        }

        self.log_verbose(&format!("Searching content in JAR: {}", jar_path.display()));

        if let Ok(file) = File::open(jar_path) {
            if let Ok(mut archive) = ZipArchive::new(file) {
                let mut counts = (0, 0, 0); // (classes, java, others)
                
                for i in 0..archive.len() {
                    if let Ok(mut file_in_zip) = archive.by_index(i) {
                        let file_name = file_in_zip.name().to_string();
                        
                        // Skip directories
                        if file_name.ends_with('/') {
                            continue;
                        }

                        // Count file types
                        if file_name.ends_with(".class") {
                            counts.0 += 1;
                        } else if file_name.ends_with(".java") {
                            counts.1 += 1;
                        } else {
                            counts.2 += 1;
                        }

                        // Check if we should search this file type
                        let should_search = file_types.contains(&"*") || 
                            (file_types.contains(&"class") && file_name.ends_with(".class")) ||
                            (file_types.contains(&"java") && file_name.ends_with(".java")) ||
                            (file_types.contains(&"other") && !file_name.ends_with(".class") && !file_name.ends_with(".java"));

                        if should_search {
                            if file_name.ends_with(".class") {
                                // For class files, use strings-like extraction for bytecode
                                self.search_in_binary_content(&mut file_in_zip, regex, jar_path, &file_name);
                            } else {
                                // For text files, search line by line
                                self.search_in_text_content(&mut file_in_zip, regex, jar_path, &file_name);
                            }
                        }
                    }
                }
                
                self.update_stats(|stats| {
                    stats.files_processed += 1;
                    stats.total_class_files += counts.0;
                    stats.total_java_files += counts.1;
                    stats.total_other_files += counts.2;
                });
            }
        }
    }

    fn search_content_in_zip(&self, zip_path: &Path, regex: &Regex) {
        if !self.should_process_file(zip_path) {
            return;
        }

        self.log_verbose(&format!("Searching content in ZIP: {}", zip_path.display()));

        if let Ok(file) = File::open(zip_path) {
            if let Ok(mut archive) = ZipArchive::new(file) {
                for i in 0..archive.len() {
                    if let Ok(mut file_in_zip) = archive.by_index(i) {
                        let file_name = file_in_zip.name().to_string();
                        
                        if !file_name.ends_with('/') {
                            self.search_in_text_content(&mut file_in_zip, regex, zip_path, &file_name);
                        }
                    }
                }
                self.update_stats(|stats| stats.files_processed += 1);
            }
        }
    }

    fn search_content_in_file(&self, file_path: &Path, regex: &Regex) {
        if !self.should_process_file(file_path) {
            return;
        }

        if let Ok(file) = File::open(file_path) {
            let reader = BufReader::new(file);
            
            for (line_num, line_result) in reader.lines().enumerate() {
                if let Ok(line) = line_result {
                    if regex.is_match(&line) {
                        let result = SearchResult {
                            file_location: file_path.display().to_string(),
                            line_number: Some(line_num + 1),
                            line_content: line.trim().to_string(),
                            match_type: self.get_file_type(file_path),
                        };
                        self.add_result(result);
                    }
                }
            }
            self.update_stats(|stats| stats.files_processed += 1);
        }
    }

    fn search_in_text_content<R: Read>(&self, reader: &mut R, regex: &Regex, archive_path: &Path, file_name: &str) {
        let mut buffer = String::new();
        if reader.read_to_string(&mut buffer).is_ok() {
            for (line_num, line) in buffer.lines().enumerate() {
                if regex.is_match(line) {
                    let result = SearchResult {
                        file_location: format!("{}:{}", archive_path.display(), file_name),
                        line_number: Some(line_num + 1),
                        line_content: line.trim().to_string(),
                        match_type: self.get_archive_file_type(file_name),
                    };
                    self.add_result(result);
                }
            }
        }
    }

    fn search_in_binary_content<R: Read>(&self, reader: &mut R, regex: &Regex, archive_path: &Path, file_name: &str) {
        let mut buffer = Vec::new();
        if reader.read_to_end(&mut buffer).is_ok() {
            // Extract strings from binary data (similar to strings command)
            let mut current_string = String::new();
            let mut in_string = false;
            
            for &byte in &buffer {
                if byte.is_ascii_graphic() || byte == b' ' || byte == b'\t' {
                    current_string.push(byte as char);
                    in_string = true;
                } else {
                    if in_string && current_string.len() >= 4 {
                        if regex.is_match(&current_string) {
                            let result = SearchResult {
                                file_location: format!("{}:{}", archive_path.display(), file_name),
                                line_number: None,
                                line_content: current_string.clone(),
                                match_type: "class_bytecode".to_string(),
                            };
                            self.add_result(result);
                        }
                    }
                    current_string.clear();
                    in_string = false;
                }
            }
            
            // Check final string
            if in_string && current_string.len() >= 4 && regex.is_match(&current_string) {
                let result = SearchResult {
                    file_location: format!("{}:{}", archive_path.display(), file_name),
                    line_number: None,
                    line_content: current_string,
                    match_type: "class_bytecode".to_string(),
                };
                self.add_result(result);
            }
        }
    }

    fn find_archive_files(&self, search_dir: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        let files: Vec<PathBuf> = WalkDir::new(search_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| !self.should_exclude_path(e.path())) // Add exclusion filter
            .filter(|e| {
                if let Some(ext) = e.path().extension() {
                    extensions.iter().any(|&target_ext| {
                        ext.to_string_lossy().to_lowercase() == target_ext.to_lowercase()
                    })
                } else {
                    false
                }
            })
            .map(|e| e.path().to_path_buf())
            .collect();

        Ok(files)
    }

    fn should_process_file(&self, file_path: &Path) -> bool {
        self.log_verbose(&format!("The size threshold is set to {} bytes", self.size_threshold));
        
        // Check exclusions first
        if self.should_exclude_path(file_path) {
            return false;
        }

        if let Ok(metadata) = file_path.metadata() {
            if (self.size_threshold == 0) {
                self.log_verbose(&format!("Processing file without size threshold: {}", file_path.display()));
                return true; // No size threshold, process all files
            }
            if metadata.len() < self.size_threshold {
                self.log_verbose(&format!("Skipping small file: {} ({} bytes)", 
                    file_path.display(), metadata.len()));
                return false;
            }
        }
        true
    }

    fn is_text_file(&self, file_path: &Path) -> bool {
        // Simple heuristic: check first few bytes
        if let Ok(mut file) = File::open(file_path) {
            let mut buffer = [0; 1024];
            if let Ok(bytes_read) = file.read(&mut buffer) {
                if bytes_read == 0 {
                    return false;
                }
                
                // Check for null bytes (binary files usually have them)
                let null_count = buffer[..bytes_read].iter().filter(|&&b| b == 0).count();
                let null_ratio = null_count as f64 / bytes_read as f64;
                
                // If more than 10% null bytes, probably binary
                return null_ratio < 0.1;
            }
        }
        false
    }

fn get_file_type(&self, file_path: &Path) -> String {
    if let Some(ext) = file_path.extension() {
        let ext_str = ext.to_string_lossy().to_lowercase();
        match ext_str.as_str() {
            "properties" => "properties_config".to_string(),
            "conf" | "config" | "cfg" => "configuration".to_string(),
            "bat" | "cmd" => "batch_script".to_string(),
            "sh" => "shell_script".to_string(),
            "xml" | "xsd" | "xsl" | "xslt" => "xml_document".to_string(),
            "json" => "json_data".to_string(),
            "yaml" | "yml" => "yaml_data".to_string(),
            "ini" => "ini_config".to_string(),
            "log" => "log_file".to_string(),
            "txt" => "text_file".to_string(),
            "md" => "markdown".to_string(),
            "py" => "python_script".to_string(),
            "rb" => "ruby_script".to_string(),
            "ps1" => "powershell_script".to_string(),
            _ => ext_str,
        }
    } else {
        "no_extension".to_string()
    }
}

    fn get_archive_file_type(&self, file_name: &str) -> String {
        if let Some(ext) = file_name.split('.').last() {
            ext.to_string()
        } else {
            "unknown".to_string()
        }
    }

    pub fn export_csv(&self, filename: &str) -> Result<(), Box<dyn std::error::Error>> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(filename)?;

        let mut writer = Writer::from_writer(file);
        writer.write_record(&["file_location", "line", "line_content", "match_type"])?;

        if let Ok(results) = self.results.lock() {
            for result in results.iter() {
                writer.write_record(&[
                    &result.file_location,
                    &result.line_number.map_or(String::new(), |n| n.to_string()),
                    &result.line_content,
                    &result.match_type,
                ])?;
            }
        }

        writer.flush()?;
        println!("{} Results exported to {}", "SUCCESS".green(), filename);
        Ok(())
    }

      pub fn print_stats(&self) {
        if let Ok(stats) = self.stats.lock() {
            let results_count = self.results.lock().map(|r| r.len()).unwrap_or(0);
            let unique_count = if self.mini_mode {
                self.unique_files.lock().map(|u| u.len()).unwrap_or(0)
            } else {
                results_count
            };

            println!("\n{}", "═══════════════════════════════════════════════════════════════".white());
            println!("{}", "                        SEARCH STATISTICS                        ".white());
            println!("{}", "═══════════════════════════════════════════════════════════════".white());
            
            println!("{:<25} {:>10}", "JAR files scanned:".cyan(), format!("{}", stats.total_jars).white());
            println!("{:<25} {:>10}", "ZIP files scanned:".cyan(), format!("{}", stats.total_zip_files).white());
            println!("{:<25} {:>10}", "Class files found:".cyan(), format!("{}", stats.total_class_files).white());
            println!("{:<25} {:>10}", "Java files found:".cyan(), format!("{}", stats.total_java_files).white());
            println!("{:<25} {:>10}", "Other files found:".cyan(), format!("{}", stats.total_other_files).white());
            println!("{:<25} {:>10}", "Total files processed:".cyan(), format!("{}", stats.files_processed).white());
            
            if self.mini_mode {
                println!("{:<25} {:>10}", "Unique files w/ matches:".cyan(), format!("{}", unique_count).green());
                println!("{:<25} {:>10}", "Total matches found:".cyan(), format!("{}", stats.matches_found).yellow());
            } else {
                println!("{:<25} {:>10}", "Matches found:".cyan(), format!("{}", results_count).green());
            }
            
            println!("{:<25} {:>10}", "Elapsed time:".cyan(), format!("{:.2}s", stats.elapsed_time.as_secs_f64()).yellow());
            
            if stats.elapsed_time.as_secs_f64() > 0.0 {
                let files_per_sec = stats.files_processed as f64 / stats.elapsed_time.as_secs_f64();
                let classes_per_sec = stats.total_class_files as f64 / stats.elapsed_time.as_secs_f64();
                println!("{:<25} {:>10}", "Files/second:".cyan(), format!("{:.2}", files_per_sec).purple());
                println!("{:<25} {:>10}", "Classes/second:".cyan(), format!("{:.2}", classes_per_sec).purple());
            }
            
            println!("{:<25} {:>10}", "Parallel jobs:".cyan(), format!("{}", self.parallel_jobs).white());
            println!("{:<25} {:>10}", "Mode:".cyan(), if self.mini_mode { "Mini (unique files)".purple() } else { "Full".white() });
            
            if !self.excludes.is_empty() {
                println!("{:<25} {:>10}", "Exclusions:".cyan(), format!("{}", self.excludes.len()).red());
                for exclude in &self.excludes {
                    println!("  {}", exclude.red());
                }
            }
            
            println!("{}", "═══════════════════════════════════════════════════════════════".white());
        }
    }


    pub fn list_jars(&self, search_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        println!("{}", "JAR Analysis Report".white());
        println!("{}", "==================".cyan());

        let jar_files = self.find_archive_files(search_dir, &["jar"])?;
        
        if jar_files.is_empty() {
            println!("{} No JAR files found in {}", "ERROR".red(), search_dir.display());
            return Ok(());
        }

        println!("{} Found {} JAR files", "INFO".blue(), jar_files.len());
        println!();

        println!("{:<50} {:>10} {:>10} {:>10} {:>10}", 
            "JAR File", "Classes", "Java", "Files", "Size (MB)");
        println!("{:<50} {:>10} {:>10} {:>10} {:>10}", 
            "--------", "-------", "----", "-----", "---------");

        let mut total_stats = (0, 0, 0, 0u64); // (classes, java, files, size)

        for jar_path in &jar_files {
            if let Ok(metadata) = jar_path.metadata() {
                let size_mb = metadata.len() as f64 / (1024.0 * 1024.0);
                let (class_count, java_count, file_count) = self.count_jar_contents(jar_path);

                let jar_name = jar_path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                
                let display_name = if jar_name.len() > 47 {
                    format!("{}...", &jar_name[..44])
                } else {
                    jar_name.to_string()
                };

                println!("{:<50} {:>10} {:>10} {:>10} {:>10.2}", 
                    display_name, class_count, java_count, file_count, size_mb);

                total_stats.0 += class_count;
                total_stats.1 += java_count;
                total_stats.2 += file_count;
                total_stats.3 += metadata.len();
            }
        }

        println!();
        println!("{:<50} {:>10} {:>10} {:>10} {:>10.2}", 
            "TOTAL", total_stats.0, total_stats.1, total_stats.2, 
            total_stats.3 as f64 / (1024.0 * 1024.0));

        Ok(())
    }

    fn count_jar_contents(&self, jar_path: &Path) -> (usize, usize, usize) {
        let mut class_count = 0;
        let mut java_count = 0;
        let mut file_count = 0;

        if let Ok(file) = File::open(jar_path) {
            if let Ok(mut archive) = ZipArchive::new(file) {
                for i in 0..archive.len() {
                    if let Ok(file_in_zip) = archive.by_index(i) {
                        let file_name = file_in_zip.name();
                        
                        if !file_name.ends_with('/') {
                            file_count += 1;
                            if file_name.ends_with(".class") {
                                class_count += 1;
                            } else if file_name.ends_with(".java") {
                                java_count += 1;
                            }
                        }
                    }
                }
            }
        }
        
        (class_count, java_count, file_count)
    }

     pub fn print_results(&self) {
        if let Ok(results) = self.results.lock() {
            if results.is_empty() {
                println!("{} No matches found", "RESULT".yellow());
                return;
            }

            println!("\n{} Found {} {}", 
                "RESULTS".green().bold(), 
                results.len(),
                if self.mini_mode { "unique files with matches" } else { "matches" }
            );
            println!("{}", "─".repeat(80).cyan());

            for (i, result) in results.iter().enumerate() {
                if self.mini_mode {
                    // Mini mode: simple file listing
                    println!("{:>3}. {}", (i + 1).to_string().white(), result.file_location.green());
                } else {
                    // Full mode: detailed results
                    if let Some(line_num) = result.line_number {
                        println!("{:>3}. {} {}:{}", 
                            (i + 1).to_string().white(),
                            result.file_location.green(),
                            "line".cyan(),
                            line_num.to_string().yellow()
                        );
                        println!("     {}: {}", 
                            result.match_type.purple(),
                            result.line_content.white()
                        );
                    } else {
                        println!("{:>3}. {} {}: {}", 
                            (i + 1).to_string().white(),
                            result.file_location.green(),
                            result.match_type.purple(),
                            result.line_content.white()
                        );
                    }
                }
            }
        }
    }

}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = Command::new("jartool")
        .version("4.0")
        .author("Rust JarTool - Ultra-fast JAR & Java analysis")
        .about("Blazing fast jar searching with Rust performance")
        .arg(Arg::new("exact_class")
            .short('c')
            .long("class")
            .value_name("CLASS_NAME")
            .help("Search for exact class name")
            .conflicts_with_all(&["class_substring", "package", "content", "method", "java_files", "java_content", "master"]))
        .arg(Arg::new("class_substring")
            .short('C')
            .long("class-contains")
            .value_name("SUBSTRING")
            .help("Search for substring in class names")
            .conflicts_with_all(&["exact_class", "package", "content", "method", "java_files", "java_content", "master"]))
        .arg(Arg::new("package")
            .short('p')
            .long("package")
            .value_name("PACKAGE")
            .help("Search by package name")
            .conflicts_with_all(&["exact_class", "class_substring", "content", "method", "java_files", "java_content", "master"]))
        .arg(Arg::new("content")
            .short('s')
            .long("search")
            .value_name("PATTERN")
            .help("Search string inside class bytecode (regex supported)")
            .conflicts_with_all(&["exact_class", "class_substring", "package", "method", "java_files", "java_content", "master"]))
        .arg(Arg::new("master")
            .short('m')
            .long("master")
            .value_name("PATTERN")
            .help("Master search: search everywhere (JAR, ZIP, Java, text files)")
            .conflicts_with_all(&["exact_class", "class_substring", "package", "content", "method", "java_files", "java_content"]))
        .arg(Arg::new("directory")
            .short('d')
            .long("dir")
            .value_name("DIR")
            .help("Directory to search in")
            .required(false)
            .default_value("."))
        .arg(Arg::new("exclude")
            .short('e')
            .long("exclude")
            .value_name("PATH")
            .help("Exclude files/paths containing this string (can be used multiple times)")
            .action(clap::ArgAction::Append))
        .arg(Arg::new("mini")
            .long("mini")
            .help("Mini mode: show only unique file names (one per file)")
            .action(clap::ArgAction::SetTrue))
        .arg(Arg::new("verbose")
            .short('v')
            .long("verbose")
            .help("Enable verbose output")
            .action(clap::ArgAction::SetTrue))
        .arg(Arg::new("size_threshold")
            .long("min-size")
            .value_name("BYTES")
            .help("Minimum file size to process")
            .default_value("0"))
        .arg(Arg::new("jobs")
            .short('j')
            .long("jobs")
            .value_name("N")
            .help("Number of parallel jobs"))
        .arg(Arg::new("export")
            .long("export")
            .value_name("FILE")
            .help("Export results to CSV file"))
        .arg(Arg::new("list_jars")
            .long("list")
            .help("List JAR files and their contents")
            .action(clap::ArgAction::SetTrue))
        .get_matches();

    let verbose = matches.get_flag("verbose");
    let mini_mode = matches.get_flag("mini");
    let size_threshold: u64 = matches.get_one::<String>("size_threshold")
        .unwrap()
        .parse()
        .unwrap_or(0);
    let parallel_jobs = matches.get_one::<String>("jobs")
        .and_then(|s| s.parse().ok());
    let search_dir = Path::new(matches.get_one::<String>("directory").unwrap());
    
    // Collect exclusion patterns
    let excludes: Vec<String> = matches.get_many::<String>("exclude")
        .unwrap_or_default()
        .map(|s| s.to_string())
        .collect();

    if !excludes.is_empty() {
        println!("{} Exclusions: {:?}", "INFO".blue(), excludes);
    }
    
    if mini_mode {
        println!("{} Mini mode enabled: showing unique files only", "MODE".purple());
    }

    let tool = JarTool::new(verbose, size_threshold, parallel_jobs, excludes, mini_mode);

    // Handle list command first
    if matches.get_flag("list_jars") {
        tool.list_jars(search_dir)?;
        return Ok(());
    }

    let mut operation_performed = false;

    // Handle search operations
    if let Some(class_name) = matches.get_one::<String>("exact_class") {
        tool.search_exact_class(class_name, search_dir)?;
        operation_performed = true;
    } else if let Some(substring) = matches.get_one::<String>("class_substring") {
        tool.search_class_substring(substring, search_dir)?;
        operation_performed = true;
    } else if let Some(package) = matches.get_one::<String>("package") {
        tool.search_package(package, search_dir)?;
        operation_performed = true;
    } else if let Some(pattern) = matches.get_one::<String>("content") {
        tool.search_content(pattern, search_dir, &["*"])?;
        operation_performed = true;
    } else if let Some(pattern) = matches.get_one::<String>("master") {
        tool.master_search(pattern, search_dir)?;
        operation_performed = true;
    }

    if !operation_performed {
        println!("{} No search operation specified. Use --help for options.", "ERROR".red());
        return Ok(());
    }

    // Print results
    tool.print_results();
    tool.print_stats();

    // Export if requested
    if let Some(export_file) = matches.get_one::<String>("export") {
        tool.export_csv(export_file)?;
    }

    Ok(())
}