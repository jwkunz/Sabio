# Sabio

## Build and Installation Instructions

### Recommended Installation for Most Users
For most users, the easiest way to install Sabio is by downloading the release binaries. You can find the latest release binaries on the [releases page](https://github.com/jwkunz/Sabio/releases).

### Building from Source
If you prefer to build Sabio from source or need to customize your installation, follow these instructions:

#### Prerequisites
- Make sure you have the following installed:
  - Git
  - A supported version of [Node.js](https://nodejs.org/)
  - [Docker](https://www.docker.com/)

#### Steps to Build
1. **Clone the Repository**:
   ```bash
   git clone https://github.com/jwkunz/Sabio.git
   cd Sabio
   ```
2. **Run the Build Script**:
   The authoritative method for building Sabio is via the provided `build.sh` script. You can execute:
   ```bash
   ./build.sh
   ```
   This script will handle downloading dependencies, running tests, and packaging the application.

3. **Post-Build Steps**:
   After the build completes, the compiled binaries will be located in the `dist/` directory:
   ```bash
   ls dist/
   ```
   You can now run the binaries or copy them to your desired location.

### Using Docker
As an alternative, you can also run Sabio using Docker. To do this, run:
```bash
docker run -it jwkunz/sabio:latest
```

### Documentation
For more detailed documentation, refer to:
- [Github Wiki](https://github.com/jwkunz/Sabio/wiki)
- [API Documentation](https://api.sabio.com/docs)

If you encounter any issues, please check the [issues page](https://github.com/jwkunz/Sabio/issues) or open a new issue with details of your problem.