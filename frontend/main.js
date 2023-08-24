const dayjs = require("dayjs");
require("dayjs/locale/hu");
const relTime = require("dayjs/plugin/relativeTime");

dayjs.locale("hu");
dayjs.extend(relTime);

globalThis.durFmt = (_one, _two) => {
    const one = dayjs(_one);
    const two = dayjs(_two);
    if (one > two) {
        return one.from(two);
    } else {
        return two.from(one);
    }
}
