FROM node:24-alpine AS webbuilder

COPY . /tibba-web
RUN apk update \
    && apk add git make \
    && cd /tibba-web \
    && npm install --force \
    && npm run  build 


FROM vicanso/static 

COPY --from=webbuilder /tibba-web/dist /static

EXPOSE 3000 

CMD ["static-serve"]

ENTRYPOINT ["/entrypoint.sh"]
