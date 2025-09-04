# JarTool

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

Ultra-fast Rust-powered tool for searching inside JAR files, ZIP archives, Java source files, and **all file types** (.properties, .bat, .conf, .xml, etc.). Perfect for code analysis, dependency hunting, and artifact inspection in large projects.

## Features

- 🚀 **Blazing Fast**: Parallel processing with Rust performance
- 📦 **Comprehensive Coverage**: JAR, ZIP, WAR, EAR archives + all file types
- 🔍 **Advanced Search**: Regex support, exact/substring matching
- 🎯 **Flexible Modes**: Full results or mini mode (unique files only)
- 🚫 **Smart Exclusions**: Exclude paths/patterns from search
- 📊 **Rich Statistics**: Detailed performance metrics and file counts
- 📄 **Export Support**: CSV export for results
- 🎨 **Colored Output**: Beautiful terminal output with colored results
- ⚡ **Binary Search**: Extracts strings from binary files (like bytecode)

## Installation

### Prerequisites
- Rust 1.70 or higher
- Linux/macOS/Windows

### Build from Source
```bash
git clone https://github.com/yourusername/jartool.git
cd jartool
cargo build --release
```

The binary will be available at `target/release/jartool`.

### Install with Cargo
```bash
cargo install --git https://github.com/yourusername/jartool.git
```

## Quick Start

### Basic Usage
```bash
# Search for a class in JAR files
./jartool --class "StringUtils" --dir /path/to/search

# Master search across all files
./jartool --master "IOException" --dir /path/to/project

# List JAR contents
./jartool --list --dir /path/to/libs
```

## Command Reference

### Search Commands

| Command | Short | Description | Example |
|---------|-------|-------------|---------|
| `--class` | `-c` | Exact class name search | `--class "ArrayList"` |
| `--class-contains` | `-C` | Substring in class names | `--class-contains "Util"` |
| `--package` | `-p` | Package name search | `--package "com.example"` |
| `--search` | `-s` | Content search in bytecode | `--search "password"` |
| `--master` | `-m` | Search everywhere (all files) | `--master "TODO"` |
| `--list` | | List JAR contents | `--list` |

### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--dir` | `-d` | Search directory | Current directory (`.`) |
| `--exclude` | `-e` | Exclude paths (can use multiple) | None |
| `--mini` | | Show only unique file names | Full results |
| `--verbose` | `-v` | Enable verbose output | Disabled |
| `--min-size` | | Minimum file size (bytes) | 0 (no limit) |
| `--jobs` | `-j` | Number of parallel jobs | CPU cores |
| `--export` | | Export results to CSV | None |

## Detailed Usage Examples

### 1. Class Name Searches

#### Exact Class Search
```bash
# Find exact class name in JAR files
./jartool --class "HashMap" --dir /opt/tomcat/lib

# With exclusions
./jartool --class "Logger" --dir /path/to/project --exclude target --exclude .git
```

#### Substring Class Search
```bash
# Find classes containing substring
./jartool --class-contains "Exception" --dir /path/to/libs

# Case-insensitive search (use regex)
./jartool --class-contains "(?i)exception" --dir /path/to/project
```

### 2. Package Searches
```bash
# Find classes in specific package
./jartool --package "org.springframework" --dir /path/to/spring/libs

# Multiple exclusions
./jartool --package "com.company" --dir /path/to/project \
  --exclude test --exclude build --exclude node_modules
```

### 3. Content Searches

#### Bytecode Content Search
```bash
# Search for strings in class bytecode
./jartool --search "password" --dir /path/to/jars

# Regex patterns
./jartool --search "jdbc:.*://" --dir /path/to/config
```

#### Master Search (All Files)
```bash
# Search across ALL file types
./jartool --master "TODO" --dir /path/to/project

# Search for configuration values
./jartool --master "server\.port" --dir /path/to/configs

# Find hardcoded IPs
./jartool --master "\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b" --dir /path/to/project
```

### 4. Mini Mode
```bash
# Show only unique files with matches (no duplicates)
./jartool --master "FIXME" --mini --dir /path/to/project

# Combine with exclusions
./jartool --master "password" --mini --dir /path/to/project \
  --exclude .git --exclude target --exclude node_modules
```

### 5. JAR Analysis
```bash
# List all JAR files and their contents
./jartool --list --dir /path/to/libs

# Analyze specific JAR
./jartool --list --dir /path/to/specific.jar
```

### 6. Export Results
```bash
# Export to CSV
./jartool --master "deprecated" --dir /path/to/project --export results.csv

# Export with mini mode
./jartool --master "TODO" --mini --dir /path/to/project --export todos.csv
```

### 7. Performance Tuning
```bash
# Use specific number of parallel jobs
./jartool --master "pattern" --jobs 8 --dir /path/to/large/project

# Set minimum file size to skip small files
./jartool --master "config" --min-size 1024 --dir /path/to/project

# Verbose output for debugging
./jartool --master "error" --verbose --dir /path/to/logs
```

## Advanced Examples

### Finding Security Issues
```bash
# Find potential hardcoded secrets
./jartool --master "(?i)(password|secret|key|token)" --dir /path/to/project --mini

# Find SQL injection vulnerabilities
./jartool --master "SELECT.*\+.*" --dir /path/to/java/src

# Find insecure configurations
./jartool --master "ssl.*false|truststore.*null" --dir /path/to/configs
```

### Code Quality Analysis
```bash
# Find TODO comments
./jartool --master "TODO|FIXME|XXX" --dir /path/to/project --mini

# Find deprecated API usage
./jartool --master "@Deprecated" --dir /path/to/java/src

# Find logging statements
./jartool --master "logger\.(debug|info|warn|error)" --dir /path/to/project
```

### Dependency Analysis
```bash
# Find specific library usage
./jartool --master "org\.apache\.commons" --dir /path/to/libs

# Find version numbers
./jartool --master "\d+\.\d+\.\d+" --dir /path/to/jars

# Find Maven coordinates
./jartool --master "groupId|artifactId|version" --dir /path/to/poms
```

## Output Examples

### Full Mode Output
```
MASTER Starting master search mode for: password
INFO Processing ALL file types (.properties, .bat, .conf, .xml, etc.)
INFO File analysis:
  JAR files: 15
  ZIP files: 3
  Java files: 42
  Config files (.properties, .conf, .ini): 8
  Script files (.bat, .sh, .py, etc.): 12
  XML files (.xml, .xsd, etc.): 5
  Text files (.txt, .json, .yaml, etc.): 23
  Other files: 7
  TOTAL Total files to process: 115

RESULTS Found 8 matches
────────────────────────────────────────────────────────────────────────────────
1. /path/to/config.properties line:15
     properties_config: db.password=secret123
2. /path/to/application.yml line:22
     yaml_data: password: ${DB_PASSWORD}
3. /path/to/MyClass.java line:45
     java: String password = "hardcoded";
```

### Mini Mode Output
```
MODE Mini mode: showing unique files only
RESULTS Found 5 unique files with matches
────────────────────────────────────────────────────────────────────────────────
1. /path/to/config.properties
2. /path/to/application.yml
3. /path/to/MyClass.java
4. /path/to/web.xml
5. /path/to/script.sh
```

### Statistics Output
```
═══════════════════════════════════════════════════════════════
                        SEARCH STATISTICS                        
═══════════════════════════════════════════════════════════════
JAR files scanned:           15
ZIP files scanned:            3
Class files found:          234
Java files found:            42
Other files found:           56
Total files processed:      115
Matches found:                8
Elapsed time:              0.45s
Files/second:              255.6
Classes/second:           520.0
Parallel jobs:                8
Mode:                    Full
Exclusions:                   3
  target
  .git
  node_modules
═══════════════════════════════════════════════════════════════
```

## File Type Support

JarTool processes **all file types** including:

- **Archives**: JAR, ZIP, WAR, EAR
- **Java**: .java, .class (bytecode)
- **Configuration**: .properties, .conf, .config, .cfg, .ini
- **Scripts**: .bat, .cmd, .sh, .py, .rb, .ps1
- **Markup**: .xml, .xsd, .xsl, .xslt, .json, .yaml, .yml
- **Text**: .txt, .md, .log
- **And more**: Any file with readable content

## Performance Tips

1. **Use Mini Mode** for large searches to reduce output
2. **Exclude unnecessary directories** (target, .git, node_modules)
3. **Set minimum file size** to skip tiny files
4. **Use specific search types** instead of master when possible
5. **Adjust parallel jobs** based on your CPU cores

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## License

MIT License - see LICENSE for details.

---

**Happy searching!** 🔍✨

For issues or feature requests, please [open an issue](https://github.com/premnath018/jartool/issues).
