import Alamofire
import UIKit

class MasterViewController: UITableViewController {
    @IBOutlet var titleImageView: UIImageView!

    var detailViewController: DetailViewController?

    override func viewDidLoad() {
        super.viewDidLoad()
        navigationItem.titleView = titleImageView
    }

    override func prepare(for segue: UIStoryboardSegue, sender: Any?) {
        if let controller = segue.destination as? DetailViewController {
            detailViewController = controller
        }
    }
}
