import Alamofire
import UIKit

class DetailViewController: UITableViewController {
    private enum Sections: Int {
        case headers, body
    }

    var request: String? {
        didSet {
            refresh()
        }
    }

    func refresh() {
        guard let request = request else { return }
        title = AF.request(request)
    }

    override func numberOfSections(in tableView: UITableView) -> Int {
        return Sections.body.rawValue + 1
    }
}
