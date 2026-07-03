# appd workerd R2 cache

This Terraform module creates the R2 bucket used by `bazel-remote` for shared
workerd Bazel build cache objects.

## Apply

```sh
terraform init
terraform plan \
  -var="cloudflare_account_id=$CLOUDFLARE_ACCOUNT_ID" \
  -var="cloudflare_api_token=$CLOUDFLARE_API_TOKEN" \
  -var="r2_access_key_id=$APPD_BAZEL_S3_ACCESS_KEY_ID" \
  -var="r2_secret_access_key=$APPD_BAZEL_S3_SECRET_ACCESS_KEY"
terraform apply \
  -var="cloudflare_account_id=$CLOUDFLARE_ACCOUNT_ID" \
  -var="cloudflare_api_token=$CLOUDFLARE_API_TOKEN" \
  -var="r2_access_key_id=$APPD_BAZEL_S3_ACCESS_KEY_ID" \
  -var="r2_secret_access_key=$APPD_BAZEL_S3_SECRET_ACCESS_KEY"
```

The Cloudflare provider creates the bucket. The AWS provider is configured
against R2's S3-compatible endpoint only to manage lifecycle rules for cached
objects and incomplete multipart uploads.

## Build Secrets

Set these values in GitHub Actions secrets, or export them locally before using
`--cache r2-read-write`:

```sh
export APPD_BAZEL_S3_ACCESS_KEY_ID="<r2-access-key-id>"
export APPD_BAZEL_S3_SECRET_ACCESS_KEY="<r2-secret-access-key>"
```
