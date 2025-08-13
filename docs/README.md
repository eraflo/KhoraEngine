# KhoraEngine Documentation Index

Welcome to the KhoraEngine documentation! This index provides an overview of all available documentation and guides.

## Table of Contents

### Getting Started
- **[Developer Guide](developer_guide.md)** - Complete guide for developers working with KhoraEngine
- **[Architecture Design](architecture_design.md)** - High-level architectural overview and SAA concepts

### Core Systems

#### Mathematics
- **[Math Module](math_module.md)** - Comprehensive guide to vectors, matrices, quaternions, and geometric types

#### Rendering
- **[Rendering System](rendering/rendering_system.md)** - Complete rendering architecture and API documentation
- **[GPU Performance Monitoring](rendering/gpu_performance_monitoring.md)** - Comprehensive GPU timing, performance hooks, and surface resize strategy

#### Memory Management
- **[Memory Management](memory_management.md)** - Memory tracking, allocation monitoring, and SAA integration

#### Event System
- **[Event System](event_system.md)** - Event-driven architecture and communication patterns

#### Performance Monitoring
- **[Performance Monitoring](performance_monitoring.md)** - Comprehensive performance tracking and metrics collection

### Development

#### Integration and Extension
- **[Integration Guide](integration_guide.md)** - How to add new subsystems and extend existing functionality

#### Development Workflow
- **[Pre-Push Verification](dev_workflow/pre_push_verification.md)** - Local verification scripts for code quality

## Quick Reference

### For New Developers
1. Start with the **[Developer Guide](developer_guide.md)** for project setup and workflow
2. Read **[Architecture Design](architecture_design.md)** to understand the engine's design philosophy
3. Follow the **[Integration Guide](integration_guide.md)** to add new features

### For Engine Users
1. Review the **[Math Module](math_module.md)** for mathematical operations
2. Study the **[Rendering System](rendering/rendering_system.md)** for graphics programming
3. Check **[Event System](event_system.md)** for communication patterns

### For Performance Optimization
1. **[Performance Monitoring](performance_monitoring.md)** - Understanding engine metrics
2. **[GPU Performance Monitoring](rendering/gpu_performance_monitoring.md)** - Comprehensive GPU timing analysis, performance hooks, and optimization strategies
3. **[Memory Management](memory_management.md)** - Memory usage and optimization

## Documentation Standards

All documentation in this project follows these standards:

### Structure
- **Table of Contents** - Every document starts with a clear TOC
- **Overview Section** - High-level explanation of the topic
- **Detailed Sections** - In-depth coverage with examples
- **Usage Examples** - Practical code examples
- **Best Practices** - Recommended patterns and anti-patterns

### Code Examples
- All code examples are **functional and tested**
- Examples include **error handling** where appropriate
- **Comments explain the "why"**, not just the "what"
- Examples progress from **simple to complex**

### Cross-References
- Links to related documentation using **relative paths**
- References to **source code** when relevant
- Links to **GitHub issues** for detailed implementation discussions

## Contributing to Documentation

### Adding New Documentation
1. Create the document in the appropriate directory
2. Follow the established structure and style
3. Add entry to this index file
4. Update any cross-references in existing documents

### Updating Existing Documentation
1. Keep the structure consistent
2. Update examples if API changes
3. Verify all links still work
4. Update version-specific information

### Documentation Review Process
1. Technical accuracy review by code owners
2. Language and clarity review
3. Example validation (code must compile and run)
4. Link verification

## Future Documentation Roadmap

### Planned Documentation
- **Shader System Guide** - WGSL shader development and pipeline creation
- **Asset System** - Asset loading, management, and streaming
- **Scene Management** - Entity-Component-System usage patterns
- **Networking Architecture** - Multiplayer and network synchronization

**Note**: Advanced adaptive architecture features (SAA) are planned for future development but not currently implemented.

## Version Information

- **Current Engine Version**: Phase 1, Milestone 2 (Active Development)
- **Documentation Version**: 1.0
- **Last Updated**: January 2025
- **Minimum Rust Version**: 1.70+

## Feedback and Contributions

### Reporting Issues
- **Documentation bugs**: Create issue with label `documentation`
- **Missing information**: Create issue with label `documentation`, `enhancement`
- **Outdated examples**: Create issue with label `documentation`, `bug`

### Contributing Improvements
- Follow the **[Contributing Guidelines](../CONTRIBUTING.md)**
- Ensure all code examples compile and run
- Test documentation builds locally
- Submit PRs with clear descriptions of changes

### Community Resources
- **GitHub Discussions**: General questions and design discussions
- **Issues**: Bug reports and specific feature requests
- **Pull Requests**: Code and documentation contributions

---

**Note**: This documentation is continuously updated as the engine evolves. For the most current information, always refer to the latest version in the main branch.

**License**: All documentation is licensed under the same terms as the KhoraEngine project (Apache License 2.0).
