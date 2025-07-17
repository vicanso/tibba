var _ = {};
_.get = (function () {
    var isArray = Array.isArray;
    /** Used to match property names within property paths. */
    var reIsDeepProp = /\.|\[(?:[^[\]]*|(["'])(?:(?!\1)[^\\]|\\.)*?\1)\]/,
        reIsPlainProp = /^\w*$/,
        reLeadingDot = /^\./,
        rePropName = /[^.[\]]+|\[(?:(-?\d+(?:\.\d+)?)|(["'])((?:(?!\2)[^\\]|\\.)*?)\2)\]|(?=(?:\.|\[\])(?:\.|\[\]|$))/g;

    function stringToPath(string) {
        var result = [];
        if (reLeadingDot.test(string)) {
            result.push('');
        }
        string.replace(rePropName, function (match, number, quote, string) {
            result.push(quote ? string.replace(reEscapeChar, '$1') : (number || match));
        });
        return result;
    };

    function castPath(value) {
        return isArray(value) ? value : stringToPath(value);
    }

    function isKey(value, object) {
        if (isArray(value)) {
            return false;
        }
        var type = typeof value;
        if (type == 'number' || type == 'symbol' || type == 'boolean' ||
            value == null) {
            return true;
        }
        return reIsPlainProp.test(value) || !reIsDeepProp.test(value) ||
            (object != null && value in Object(object));
    }
    function baseGet(object, path) {
        path = isKey(path, object) ? [path] : castPath(path);

        var index = 0,
            length = path.length;

        while (object != null && index < length) {
            object = object[path[index++]];
        }
        return (index && index == length) ? object : undefined;
    }

    function get(object, path, defaultValue) {
        var result = object == null ? undefined : baseGet(object, path);
        return result === undefined ? defaultValue : result;
    }
    return get;
})();