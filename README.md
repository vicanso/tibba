# tibba


## postgres

```bash
docker pull postgres:18-alpine

docker run -d --restart=always \
  -v $PWD/postgres:/var/lib/postgresql \
  -e POSTGRES_PASSWORD=A123456 \
  -p 5432:5432 \
  --name=cybertect-postgres \
  postgres:18-alpine

docker exec -it cybertect-postgres sh

psql -c "CREATE DATABASE cybertect;" -U postgres
psql -c "CREATE USER vicanso WITH PASSWORD 'A123456';" -U postgres
psql -c "GRANT ALL PRIVILEGES ON DATABASE cybertect to vicanso;" -U postgres
psql -c "GRANT ALL ON DATABASE cybertect TO vicanso;" -U postgres
psql -c "ALTER DATABASE cybertect OWNER TO vicanso;" -U postgres
```