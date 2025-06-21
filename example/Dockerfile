FROM openjdk:11-jre-slim

RUN apt-get update && apt-get install -y \
    tini \
    curl \
    unzip

ENV VERSION=2.34.0

RUN curl -o allure-commandline.zip -L https://github.com/allure-framework/allure2/releases/download/$VERSION/allure-$VERSION.zip \
    && unzip allure-commandline.zip -d /opt/ \
    && rm allure-commandline.zip \
    && ln -s /opt/allure-$VERSION/bin/allure /usr/bin/allure

WORKDIR /app

EXPOSE 4040

ENTRYPOINT ["/usr/bin/tini", "--"]
