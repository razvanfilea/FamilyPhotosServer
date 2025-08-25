# FamilyPhotos Server

An open source self-hosted photo and video server for your family written in Rust.

## How to set up

Build the docker image, set up the environment variables and run the image.<br>

It's expected that you run a proxy like Nginx to handle TLS if you need it.

### Docker

Clone the repository and run the following command to build a docker image:

```shell
docker build -t familyphotos .
```

### Docker Compose

Here is an example of a Docker compose file

```
services:
   familyphotos:
     container_name: familyphotos
     image: familyphotos
     volumes:
       - /path/to/photos/folder/:/mnt/photos/
     restart: always
     ports:
        8080:3000
     environment:
       SERVER_PORT: 3000
       STORAGE_PATH: /mnt/photos/
```

Below you can see all the environment variables that can be configured

### Env Variables

Variables in bold **must** be specified.

- **SERVER_PORT**: The port the server should listen on
- **STORAGE_PATH**: The path to the folder where the photos will be stored
- DATABASE_URL: Alternative path for the database.
  Must have the format "sqlite:://path/to/database.db" [default: in ${STORAGE_PATH}/.familyphotos.db]
- PREVIEWS_PATH: Alternative storage path for photo previews (this, for example is useful when you want to store the
  photos on an HDD but the previews on an SSD) [default: in ${STORAGE_PATH}/.preview]
- SCAN_NEW_FILES: Scan the storage for external changes at startup and periodically [default: true]
- BACKGROUND_THREADS_COUNT: Number of threads to use for background tasks [default: number of logical CPUs]

### Creating user accounts

To create new user accounts, run the following command using the CLI:<br>

```shell
familyphotos user create -u <user_name> -d <display_name> [-p <password>]
```

This will generate a new user with the given username, display name and password or a random one if not provided.<br>

### Example Nginx Config with HTTPS

```
http {
    upstream familyphotos {
        zone upstreams 1M;
        server 127.0.0.1:3000;
        keepalive 2;
    }

    server {
        # HTTPS Config
        listen 443 ssl http2;
        listen [::]:443 ssl http2;
        ssl_certificate /path/to/certificate.pem;
        ssl_certificate_key /path/to/privkey.pem;
        ssl_protocols TLSv1.2 TLSv1.3;

        server_name example.com;

        # Recommended for Performance (optional)
        sendfile on;
        tcp_nopush on;
        tcp_nodelay on;
        proxy_http_version 1.1;
        proxy_set_header "Connection" "";
        proxy_buffers 4 2M;
        proxy_buffer_size 1M;
        proxy_busy_buffers_size 2M;

        
        location ~ ^/(.*)$ {
            client_max_body_size 1G;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_pass http://familyphotos/$1$is_args$args;
            proxy_redirect off;
        }
    }
}
```

## Folder structure

The server will generate the following folder structure in the STORAGE_PATH folder:

```
├───.familyphotos.db # Database (if not specified elsewhere)
│
├───.previews/ # Folder for previews (if not specified elsewhere)
│
├───public/ # The folder of the "public" user, alas photos who belong to everyone
│   ├───<album_name>/ # Folder for albums aka "folders"
│   │   └───<photo_name> # Photo files
│   └───<photo_name> # Photo files
│
└───<user_name>/ # Folder for each individual user
    ├───<album_name>/ # Folder for albums aka "folders"
    │   └───<photo_name> # Photo files
    └───<photo_name> # Photo files
```
