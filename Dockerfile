# for this build to work, copy the latest build from the github action
# into the builds folder (and extract it) so you have a './build/downloader'
# file in there. Then run 'docker compose build' and 'docker compose up -d' to
# build and start/update the containers

# create a small container to start the binary
FROM alpine:latest
# add libgcc to the container (it needs it for some reason)
RUN apk add libgcc
# add ffmpeg to the container (needed for video splitting)
RUN apk add ffmpeg

COPY ./logger.yaml ./logger.yaml

# copy the binary from the build folder
# this binary should be build with the
# target x86_64-unknown-linux-musl to be able to run
COPY ./build/ /bin/
# make sure the binary is executable
RUN chmod +x /bin/downloader

# set the start cmd
CMD ["/bin/downloader"]

# below are some command to run if something goes wrong
# and the file system has to be checked etc.
#CMD ["/bin/sh"]
#CMD echo $TWITCH_CLIENT_ID $TWITCH_CLIENT_SECRET
