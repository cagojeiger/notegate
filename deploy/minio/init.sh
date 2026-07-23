#!/bin/sh
set -eu

if [ "$MINIO_ROOT_USER" = "$NOTEGATE_S3_ACCESS_KEY" ]; then
  echo "MinIO root and NoteGate access keys must differ" >&2
  exit 1
fi

mc alias set local "${MINIO_ENDPOINT:-http://minio:9000}" "$MINIO_ROOT_USER" "$MINIO_ROOT_PASSWORD"
mc mb --ignore-existing "local/$NOTEGATE_S3_BUCKET"
mc anonymous set none "local/$NOTEGATE_S3_BUCKET"

mc admin user add local "$NOTEGATE_S3_ACCESS_KEY" "$NOTEGATE_S3_SECRET_KEY"
mc admin policy create local notegate-app /dev/stdin <<POLICY
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "s3:GetObject",
        "s3:PutObject",
        "s3:DeleteObject",
        "s3:AbortMultipartUpload"
      ],
      "Resource": "arn:aws:s3:::$NOTEGATE_S3_BUCKET/objects/*"
    }
  ]
}
POLICY
mc admin policy attach local notegate-app --user "$NOTEGATE_S3_ACCESS_KEY"
