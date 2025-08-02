# Feature Overview

Authentication for HTTP API calls.

## Related Headers

- **x-ne-auth-additional-headers**: Additional headers included in the signature calculation. Included in the signature calculation. It is allowed to be missing in the request, but its value should be treated as empty string in the signature calculation.
- **x-ne-auth-algo**: The algorithm used for the signature, Included in the signature calculation. Values: sha256-rsa.
- **x-ne-auth-biz-code**: The business code representing the entity that signs the request/response. Included in the signature calculation.
- **x-ne-auth-sha256sum**: The sha256 sum of the request/response body. Included in the signature calculation.
- **x-ne-auth-sign**: The signature for the request/response.
- **x-ne-request-id**: The request id. It should be unique in 24 hours for the same biz code for non-GET request. Included in the signature calculation. 
- **x-ne-response-id**: The id of the response. It will only be appeared in the response. Included in the signature calculation of response. We use a milli second timestamp for this id. Currently, we don't guarantee the uniqueness and the format of the id. It is no more than of a tag of the response.

## Sign Request

### To Be Sign Data

```
{URL_PATH}\n{SORTED_QUERY_STRING_KEY_PAIRS}\n{HTTP_METHOD}\n{SORTED_INCLUDED_HTTP_HEADER_KEY_PAIRS}
```

- **URL_PATH**: The url path of the api. For example, the url path of 'https://example.com/api/v1/user' is '/api/v1/user'.
- **SORTED_QUERY_STRING_KEY_PAIRS**: For all params of query string, sort the param key with dictionary order, join key and value with '=' without encoding and then join all key paris with '&'. For example, the query string 'name=ferret&color=purple%28' will be 'color=purple(&name=ferret'.
- **SORTED_INCLUDED_HTTP_HEADER_KEY_PAIRS**: For all headers, sort the header with dictionary order, join the header name and the value with '=' and then join all key pairs with '&'. Header name should be all in lower case. Must included headers: **x-ne-auth-additional-headers**, **x-ne-auth-algo**, **x-ne-auth-biz-code**, **x-ne-auth-sha256sum**, **x-ne-request-id**.

## Sign Response

### To Be Sign Data

```
HTTP_STATUS_CODE\n{SORTED_INCLUDED_HTTP_HEADER_KEY_PAIRS}
```

- **SORTED_INCLUDED_HTTP_HEADER_KEY_PAIRS**: For all headers, sort the header with dictionary order, join the header name and the value with '=' and then join all key pairs with '&'. Header name should be all in lower case. Must included headers: **x-ne-auth-additional-headers**, **x-ne-auth-algo**, **x-ne-auth-biz-code**, **x-ne-auth-sha256sum**, **x-ne-request-id**, **x-ne-response-id**.

The value of **x-ne-auth-algo** and **x-ne-request-id** will be the same as the one in the request. And **x-ne-auth-biz-code** will be `ne-v1`.
